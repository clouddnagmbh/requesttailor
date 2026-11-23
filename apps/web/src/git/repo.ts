// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The Git operations of the web app, on isomorphic-git, independent of the browser: they get
// an `fs` (./fs.ts in the browser, Node's fs in unit tests) and, for the network, an `http`
// client (./http.ts) plus a credentials lookup. ./index.ts adds the workspace rules (unsaved
// changes, reopening the collection) and turns this into `GitApi`.
//
// Model (rt_app::git): no staging area. `commit` builds its tree from HEAD plus exactly the
// given paths and updates the index only for those, so whatever else is staged (e.g. from the
// command line) stays as it is. A pull that stops on conflicts leaves Git's own merge state:
// conflict markers in the files, the conflicting stages in the index and `.git/MERGE_HEAD`,
// so the command line sees the same merge. `resolve` takes one side; the next `commit`
// finishes the merge (two parents).
//
// Wave 12 (strand B): the pull merges the collection on the model. Before isomorphic-git
// merges, the core merges the collection's files of base, ours and theirs (`../merges.ts`,
// rt-wasm); isomorphic-git's merge driver answers with that result, and files outside the
// collection get the core's line merge one at a time. A file with field conflicts is left
// conflicted with ours in each conflicted field and a comment above it. After a stopped merge,
// files the model merges cleanly (e.g. a request one side renamed) are written and marked
// resolved; if nothing is left, the merge commit is made. `mergeSides` and `writeResolved`
// serve `MergeApi`.

import "./buffer-global";

import git from "isomorphic-git";
import type { HttpClient as IsoHttpClient } from "isomorphic-git";

import {
  BackendError,
  type GitBranch,
  type GitCommit,
  type GitConflict,
  type GitFile,
  type GitIdentity,
  type GitPullResult,
  type GitResolution,
  type GitStatus,
} from "@requesttailor/ui";

import { collectionMerger, type CollectionMerge, type CollectionMerger, type ConflictStages, type TextFiles } from "../merges";
import type { FsClient } from "./fs";
import { hostOf, type HttpClient } from "./http";
import { treeAt } from "./revisions";
import { collectionPath, repoPath, scopedPath } from "./scope";

export interface Repo {
  fs: FsClient;
  /** Working tree root as isomorphic-git sees it (`/` in the browser). */
  dir: string;
  /** isomorphic-git's object cache, for one operation. */
  cache: object;
  /** The collection's folder in the repository (`a/b`), `""` or missing when the collection
   *  is the work tree. The paths of `status`, `commit`, `diffSides`, the merge and
   *  `treeAt`/`show` are relative to it (a leading `/`: relative to the repository); the
   *  wave-14 parts (./actions.ts) use repository paths. */
  scope?: string;
}

export interface Credentials {
  username: string;
  password: string;
}

export interface Network {
  http: HttpClient;
  /** Stored credentials for `host`, or `null`. */
  credentials(host: string): Promise<Credentials | null>;
  onProgress?: (phase: string, loaded: number, total: number | undefined) => void;
}

export const NOT_A_REPO: GitStatus = {
  repo: false,
  branch: null,
  upstream: null,
  ahead: 0,
  behind: 0,
  files: [],
  merging: false,
};

/** How far ahead/behind is counted; beyond this, the count stops. */
const COUNT_LIMIT = 10_000;

const gitErr = (message: string) => new BackendError("git", message);

function path(repo: Repo, rel: string): string {
  return `${repo.dir.replace(/\/+$/, "")}/${rel}`;
}

const gitdir = (repo: Repo) => path(repo, ".git");

function base(repo: Repo) {
  return { fs: repo.fs, dir: repo.dir, cache: repo.cache };
}

async function readOptional(repo: Repo, rel: string): Promise<Uint8Array | null> {
  try {
    return (await repo.fs.promises.readFile(path(repo, rel))) as Uint8Array;
  } catch (e) {
    const code = (e as { code?: string }).code;
    if (code === "ENOENT" || code === "ENOTDIR" || code === "EISDIR") return null;
    throw e;
  }
}

async function removeOptional(repo: Repo, rel: string): Promise<void> {
  try {
    await repo.fs.promises.unlink(path(repo, rel));
  } catch (e) {
    if ((e as { code?: string }).code !== "ENOENT") throw e;
  }
}

/** Writes a working-tree file, creating its folders. */
async function writeWorkFile(repo: Repo, rel: string, data: Uint8Array | string): Promise<void> {
  const parts = rel.split("/");
  let dir = repo.dir.replace(/\/+$/, "");
  for (const part of parts.slice(0, -1)) {
    dir = `${dir}/${part}`;
    try {
      await repo.fs.promises.mkdir(dir);
    } catch (e) {
      if ((e as { code?: string }).code !== "EEXIST") throw e;
    }
  }
  await repo.fs.promises.writeFile(path(repo, rel), data);
}

export async function isRepo(repo: Repo): Promise<boolean> {
  return (await readOptional(repo, ".git/HEAD")) !== null;
}

/** HEAD's commit, or `null` in a repository without commits. */
async function headOid(repo: Repo): Promise<string | null> {
  try {
    return await git.resolveRef({ fs: repo.fs, dir: repo.dir, ref: "HEAD" });
  } catch {
    return null;
  }
}

async function refOid(repo: Repo, ref: string): Promise<string | null> {
  try {
    return await git.resolveRef({ fs: repo.fs, dir: repo.dir, ref });
  } catch {
    return null;
  }
}

async function mergeHead(repo: Repo): Promise<string | null> {
  const bytes = await readOptional(repo, ".git/MERGE_HEAD");
  if (!bytes) return null;
  const oid = new TextDecoder().decode(bytes).trim().split(/\s+/)[0];
  return /^[0-9a-f]{40}$/.test(oid) ? oid : null;
}

/** Paths with conflict stages (1–3) in `.git/index` (index versions 2–4). */
export function unmergedPaths(index: Uint8Array): string[] {
  const view = new DataView(index.buffer, index.byteOffset, index.byteLength);
  if (index.length < 12 || new TextDecoder().decode(index.subarray(0, 4)) !== "DIRC") return [];
  const version = view.getUint32(4);
  const count = view.getUint32(8);
  const out = new Set<string>();
  let at = 12;
  let previous = "";
  const decoder = new TextDecoder();
  for (let i = 0; i < count; i++) {
    const start = at;
    const flags = view.getUint16(at + 60);
    at += 62;
    if (version >= 3 && flags & 0x4000) at += 2;
    let name: string;
    if (version === 4) {
      // Prefix compression: a varint of bytes to drop from the previous name, then the rest.
      let strip = index[at] & 0x7f;
      while (index[at++] & 0x80) strip = ((strip + 1) << 7) | (index[at] & 0x7f);
      const end = index.indexOf(0, at);
      name = previous.slice(0, previous.length - strip) + decoder.decode(index.subarray(at, end));
      at = end + 1;
    } else {
      const end = index.indexOf(0, at);
      name = decoder.decode(index.subarray(at, end));
      at = start + ((end - start + 8) & ~7);
    }
    previous = name;
    if ((flags >> 12) & 3) out.add(name);
  }
  return [...out].sort();
}

async function conflicted(repo: Repo): Promise<string[]> {
  const index = await readOptional(repo, ".git/index");
  return index ? unmergedPaths(index) : [];
}

interface Upstream {
  remote: string;
  /** Branch name on the remote. */
  branch: string;
}

async function upstreamOf(repo: Repo, branch: string): Promise<Upstream | null> {
  const remote = (await git.getConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${branch}.remote` })) as
    | string
    | undefined;
  const merge = (await git.getConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${branch}.merge` })) as
    | string
    | undefined;
  if (!remote || !merge) return null;
  return { remote, branch: merge.replace(/^refs\/heads\//, "") };
}

/** Commits reachable from `from` but not from `not`. */
async function countOnly(repo: Repo, from: string, not: string | null): Promise<number> {
  const seen = new Set<string>();
  const stack = not ? [not] : [];
  while (stack.length > 0 && seen.size < COUNT_LIMIT) {
    const oid = stack.pop()!;
    if (seen.has(oid)) continue;
    seen.add(oid);
    stack.push(...(await git.readCommit({ ...base(repo), oid })).commit.parent);
  }
  let count = 0;
  const done = new Set<string>();
  const todo = [from];
  while (todo.length > 0 && count < COUNT_LIMIT) {
    const oid = todo.pop()!;
    if (seen.has(oid) || done.has(oid)) continue;
    done.add(oid);
    count++;
    todo.push(...(await git.readCommit({ ...base(repo), oid })).commit.parent);
  }
  return count;
}

/** The status with the collection's files only (paths relative to it), like `rt-git`. */
export async function status(repo: Repo): Promise<GitStatus> {
  const s = await repoStatus(repo);
  if (!repo.scope || !s.repo) return s;
  const files: GitFile[] = [];
  for (const f of s.files) {
    const p = scopedPath(repo, f.path);
    if (p !== null) files.push({ ...f, path: p });
  }
  return { ...s, files };
}

/** The status of the whole work tree (repository paths). */
async function repoStatus(repo: Repo): Promise<GitStatus> {
  if (!(await isRepo(repo))) return NOT_A_REPO;
  const head = await headOid(repo);
  const current = (await git.currentBranch({ fs: repo.fs, dir: repo.dir, fullname: false })) ?? null;
  const branch = head ? current : null;
  let upstream: string | null = null;
  let ahead = 0;
  let behind = 0;
  if (current) {
    const up = await upstreamOf(repo, current);
    if (up) {
      upstream = `${up.remote}/${up.branch}`;
      const theirs = await refOid(repo, `refs/remotes/${up.remote}/${up.branch}`);
      if (head && theirs && theirs !== head) {
        ahead = await countOnly(repo, head, theirs);
        behind = await countOnly(repo, theirs, head);
      }
    }
  }
  const unmerged = new Set(await conflicted(repo));
  const files: GitFile[] = [];
  const matrix = await git.statusMatrix({ ...base(repo), ...(repo.scope ? { filepaths: [repo.scope] } : {}) });
  for (const [filepath, h, w] of matrix) {
    if (unmerged.has(filepath)) files.push({ path: filepath, state: "conflicted" });
    else if (h === 0 && w !== 0) files.push({ path: filepath, state: "untracked" });
    else if (h === 1 && w === 0) files.push({ path: filepath, state: "deleted" });
    else if (h === 1 && w === 2) files.push({ path: filepath, state: "modified" });
  }
  for (const p of unmerged) {
    if (!files.some((f) => f.path === p) && (!repo.scope || scopedPath(repo, p) !== null)) {
      files.push({ path: p, state: "conflicted" });
    }
  }
  files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return { repo: true, branch, upstream, ahead, behind, files, merging: (await mergeHead(repo)) !== null };
}

async function blobAt(repo: Repo, commit: string | null, filepath: string): Promise<Uint8Array | null> {
  if (!commit) return null;
  try {
    return (await git.readBlob({ ...base(repo), oid: commit, filepath })).blob;
  } catch {
    return null;
  }
}

/** HEAD and working-tree bytes of a file (`null` where missing), for the diff. */
export async function diffSides(repo: Repo, path: string): Promise<[Uint8Array | null, Uint8Array | null]> {
  const filepath = repoPath(repo, path);
  return [await blobAt(repo, await headOid(repo), filepath), await readOptional(repo, filepath)];
}

interface Entry {
  mode: string;
  oid: string;
  type: "blob" | "tree" | "commit";
}

/** Flat map path → entry of a commit's tree (blobs and submodules only). */
async function flatTree(repo: Repo, commit: string | null): Promise<Map<string, Entry>> {
  const out = new Map<string, Entry>();
  if (!commit) return out;
  const walk = async (oid: string, prefix: string) => {
    const { tree } = await git.readTree({ ...base(repo), oid });
    for (const e of tree) {
      const p = prefix ? `${prefix}/${e.path}` : e.path;
      if (e.type === "tree") await walk(e.oid, p);
      else out.set(p, { mode: e.mode, oid: e.oid, type: e.type });
    }
  };
  const { commit: c } = await git.readCommit({ ...base(repo), oid: commit });
  await walk(c.tree, "");
  return out;
}

/** Writes nested tree objects for a flat map and returns the root tree's oid. */
async function writeFlatTree(repo: Repo, entries: Map<string, Entry>): Promise<string> {
  interface Node {
    files: Map<string, Entry>;
    dirs: Map<string, Node>;
  }
  const root: Node = { files: new Map(), dirs: new Map() };
  for (const [p, e] of entries) {
    const parts = p.split("/");
    let node = root;
    for (const part of parts.slice(0, -1)) {
      let next = node.dirs.get(part);
      if (!next) node.dirs.set(part, (next = { files: new Map(), dirs: new Map() }));
      node = next;
    }
    node.files.set(parts[parts.length - 1], e);
  }
  const write = async (node: Node): Promise<string> => {
    const tree: { mode: string; path: string; oid: string; type: "blob" | "tree" | "commit" }[] = [];
    for (const [name, sub] of node.dirs) tree.push({ mode: "040000", path: name, oid: await write(sub), type: "tree" });
    for (const [name, e] of node.files) tree.push({ mode: e.mode, path: name, oid: e.oid, type: e.type });
    return git.writeTree({ fs: repo.fs, dir: repo.dir, tree });
  };
  return write(root);
}

function toCommit(oid: string, c: { message: string; author: { name: string; timestamp: number } }): GitCommit {
  return { oid, summary: c.message.split("\n")[0], author: c.author.name, time: c.author.timestamp };
}

/** Commits the collection files `paths` (relative to the collection). */
export function commit(repo: Repo, message: string, paths: string[], identity: GitIdentity): Promise<GitCommit> {
  return commitPaths(repo, message, paths.map((p) => repoPath(repo, p)), identity);
}

async function commitPaths(
  repo: Repo,
  message: string,
  paths: string[],
  identity: GitIdentity,
): Promise<GitCommit> {
  if (!message.trim()) throw gitErr("Write a commit message.");
  if (paths.length === 0) throw gitErr("Select at least one file to commit.");
  if (!(await isRepo(repo))) throw gitErr("The collection is not in a Git repository.");
  const head = await headOid(repo);
  const merging = await mergeHead(repo);
  const unmerged = await conflicted(repo);
  const left = unmerged.filter((p) => paths.includes(p) || merging);
  if (left.length > 0) throw gitErr(`Resolve the conflict in ${left.join(", ")} first.`);

  const entries = await flatTree(repo, head);
  const before = head ? (await git.readCommit({ ...base(repo), oid: head })).commit.tree : null;
  const present: string[] = [];
  const gone: string[] = [];
  for (const p of [...new Set(paths)]) {
    const bytes = await readOptional(repo, p);
    if (bytes) {
      const oid = await git.writeBlob({ fs: repo.fs, dir: repo.dir, blob: bytes });
      entries.set(p, { mode: entries.get(p)?.mode ?? "100644", oid, type: "blob" });
      present.push(p);
    } else {
      entries.delete(p);
      gone.push(p);
    }
  }
  const tree = await writeFlatTree(repo, entries);
  if (tree === before && !merging) throw gitErr("The selected files have no changes to commit.");
  const author = { name: identity.name, email: identity.email };
  const parent = [...(head ? [head] : []), ...(merging ? [merging] : [])];
  const oid = await git.commit({ ...base(repo), message, author, committer: author, tree, parent });
  for (const p of present) await git.add({ ...base(repo), filepath: p });
  for (const p of gone) await git.remove({ fs: repo.fs, dir: repo.dir, filepath: p });
  if (merging) {
    await removeOptional(repo, ".git/MERGE_HEAD");
    await removeOptional(repo, ".git/MERGE_MSG");
  }
  const { commit: c } = await git.readCommit({ ...base(repo), oid });
  return toCommit(oid, c);
}

export async function log(repo: Repo, limit: number): Promise<GitCommit[]> {
  if (!(await isRepo(repo)) || !(await headOid(repo))) return [];
  const entries = await git.log({ ...base(repo), depth: Math.max(1, limit) });
  return entries.map((e) => toCommit(e.oid, e.commit));
}

export async function branches(repo: Repo): Promise<GitBranch[]> {
  if (!(await isRepo(repo))) return [];
  const current = await git.currentBranch({ fs: repo.fs, dir: repo.dir, fullname: false });
  const out: GitBranch[] = (await git.listBranches({ fs: repo.fs, dir: repo.dir })).map((name) => ({
    name,
    current: name === current,
    remote: false,
  }));
  if (current && !out.some((b) => b.name === current)) out.unshift({ name: current, current: true, remote: false });
  for (const { remote } of await git.listRemotes({ fs: repo.fs, dir: repo.dir })) {
    for (const name of await git.listBranches({ fs: repo.fs, dir: repo.dir, remote })) {
      if (name !== "HEAD") out.push({ name: `${remote}/${name}`, current: false, remote: true });
    }
  }
  return out;
}

async function requireNoMerge(repo: Repo, what: string): Promise<void> {
  if (await mergeHead(repo)) throw gitErr(`Finish the merge (resolve and commit) before you ${what}.`);
}

export async function checkout(repo: Repo, branch: string): Promise<void> {
  await requireNoMerge(repo, "switch branches");
  let local = branch;
  const locals = await git.listBranches({ fs: repo.fs, dir: repo.dir });
  if (!locals.includes(branch)) {
    const remotes = (await git.listRemotes({ fs: repo.fs, dir: repo.dir })).map((r) => r.remote);
    const remote = remotes.find((r) => branch.startsWith(`${r}/`));
    if (!remote) throw gitErr(`There is no branch ${branch}.`);
    local = branch.slice(remote.length + 1);
    const oid = await refOid(repo, `refs/remotes/${remote}/${local}`);
    if (!oid) throw gitErr(`There is no branch ${branch}.`);
    if (!locals.includes(local)) {
      await git.writeRef({ fs: repo.fs, dir: repo.dir, ref: `refs/heads/${local}`, value: oid });
      await git.setConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${local}.remote`, value: remote });
      await git.setConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${local}.merge`, value: `refs/heads/${local}` });
    }
  }
  await git.checkout({ ...base(repo), ref: local });
}

export async function createBranch(repo: Repo, name: string, checkoutNow: boolean): Promise<void> {
  const trimmed = name.trim();
  if (!trimmed) throw gitErr("Name the branch.");
  if ((await git.listBranches({ fs: repo.fs, dir: repo.dir })).includes(trimmed)) {
    throw gitErr(`A branch named ${trimmed} already exists.`);
  }
  if (!(await headOid(repo))) {
    if (!checkoutNow) throw gitErr("Commit something first: a new repository has no commit to branch from.");
    // No commit yet: only HEAD moves to the new (unborn) branch.
    await git.writeRef({
      fs: repo.fs,
      dir: repo.dir,
      ref: "HEAD",
      value: `refs/heads/${trimmed}`,
      symbolic: true,
      force: true,
    });
    return;
  }
  await git.branch({ fs: repo.fs, dir: repo.dir, ref: trimmed, checkout: checkoutNow });
}

export async function init(repo: Repo): Promise<void> {
  if (await isRepo(repo)) throw gitErr("The collection is already a Git repository.");
  await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
  if (!(await isRepo(repo))) {
    // isomorphic-git skips a `.git` that already has a config (e.g. a stray `.git/config`):
    // add what makes it a repository.
    for (const d of ["objects", "objects/info", "objects/pack", "refs", "refs/heads", "refs/tags"]) {
      try {
        await repo.fs.promises.mkdir(path(repo, `.git/${d}`));
      } catch (e) {
        if ((e as { code?: string }).code !== "EEXIST") throw e;
      }
    }
    await repo.fs.promises.writeFile(path(repo, ".git/HEAD"), "ref: refs/heads/main\n");
  }
}

async function remoteUrl(repo: Repo, remote: string): Promise<string> {
  const url = (await git.getConfig({ fs: repo.fs, dir: repo.dir, path: `remote.${remote}.url` })) as
    | string
    | undefined;
  if (!url) throw gitErr(`There is no remote named ${remote}. Add one on the command line (git remote add).`);
  return url;
}

/** isomorphic-git's auth callbacks for `url`: stored credentials once, then `git-auth`. */
function auth(net: Network, url: string) {
  const host = hostOf(url);
  let tried = false;
  return {
    host,
    onAuth: async () => {
      if (tried) return { cancel: true };
      tried = true;
      const c = await net.credentials(host);
      return c ? { username: c.username, password: c.password } : { cancel: true };
    },
    onAuthFailure: () => ({ cancel: true }),
  };
}

function onProgress(net: Network) {
  return net.onProgress
    ? (p: { phase: string; loaded: number; total?: number }) => net.onProgress!(p.phase, p.loaded, p.total)
    : undefined;
}

/** Turns errors of an isomorphic-git operation into `BackendError`s. */
export function gitError(e: unknown, host?: string): BackendError {
  if (e instanceof BackendError) return e;
  const err = e as { code?: string; message?: string; data?: Record<string, unknown> };
  const status = (err.data as { statusCode?: number } | undefined)?.statusCode;
  if (host && (err.code === "UserCanceledError" || (err.code === "HttpError" && (status === 401 || status === 403)))) {
    return new BackendError("git-auth", host);
  }
  switch (err.code) {
    case "PushRejectedError":
      return gitErr("The remote has commits that are not here yet. Pull first, then push again.");
    case "CheckoutConflictError": {
      const files = (err.data as { filepaths?: string[] } | undefined)?.filepaths ?? [];
      return gitErr(`Your uncommitted changes to ${files.join(", ") || "some files"} would be overwritten. Commit them first.`);
    }
    case "MergeNotSupportedError":
      return gitErr("This merge is too complex for the web app (e.g. a file deleted on one side and changed on the other). Use the desktop app or the command line.");
    case "HttpError":
      return gitErr(`The Git server answered HTTP ${status ?? "error"}${host ? ` (${host})` : ""}.`);
    case "SmartHttpError":
      return gitErr(`${host ?? "The server"} does not look like a Git server (smart HTTP).`);
    case "NotFoundError":
    case "InvalidRefNameError":
    case "AlreadyExistsError":
      return gitErr(err.message ?? String(e));
    default:
      return new BackendError("git", err.message ?? String(e));
  }
}

export async function push(repo: Repo, net: Network): Promise<void> {
  await requireNoMerge(repo, "push");
  const branch = await git.currentBranch({ fs: repo.fs, dir: repo.dir, fullname: false });
  if (!branch || !(await headOid(repo))) throw gitErr("Commit on a branch before you push.");
  const up = await upstreamOf(repo, branch);
  const remote = up?.remote ?? "origin";
  const url = await remoteUrl(repo, remote);
  const a = auth(net, url);
  const remoteRef = up?.branch ?? branch;
  try {
    const result = await git.push({
      ...base(repo),
      http: net.http as unknown as IsoHttpClient,
      remote,
      ref: branch,
      remoteRef: `refs/heads/${remoteRef}`,
      onAuth: a.onAuth,
      onAuthFailure: a.onAuthFailure,
      onProgress: onProgress(net),
    });
    const refError = Object.values(result.refs ?? {}).find((r) => !r.ok)?.error;
    if (!result.ok || refError) {
      const why = refError ?? result.error ?? "rejected";
      if (/non-fast-forward|fetch first|rejected/i.test(why)) {
        throw gitErr("The remote has commits that are not here yet. Pull first, then push again.");
      }
      throw gitErr(`The remote refused the push: ${why}`);
    }
  } catch (e) {
    throw gitError(e, a.host);
  }
  const head = (await headOid(repo))!;
  await git.writeRef({ fs: repo.fs, dir: repo.dir, ref: `refs/remotes/${remote}/${remoteRef}`, value: head, force: true });
  if (!up) {
    await git.setConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${branch}.remote`, value: remote });
    await git.setConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${branch}.merge`, value: `refs/heads/${remoteRef}` });
  }
}

/** Paths whose content differs between two flat trees. */
function changedPaths(a: Map<string, Entry>, b: Map<string, Entry>): Set<string> {
  const out = new Set<string>();
  for (const [p, e] of a) if (b.get(p)?.oid !== e.oid) out.add(p);
  for (const p of b.keys()) if (!a.has(p)) out.add(p);
  return out;
}

/** Fails if local changes (anything not committed) touch `paths`. */
async function guardLocalChanges(repo: Repo, paths: Set<string>): Promise<void> {
  const local = (await repoStatus(repo)).files.map((f) => f.path).filter((p) => paths.has(p));
  if (local.length > 0) {
    throw gitErr(`Commit your changes to ${local.join(", ")} before pulling: the pull changes these files too.`);
  }
}

export async function pull(repo: Repo, net: Network, identity: GitIdentity | null): Promise<GitPullResult> {
  await requireNoMerge(repo, "pull");
  const branch = await git.currentBranch({ fs: repo.fs, dir: repo.dir, fullname: false });
  if (!branch) throw gitErr("Switch to a branch before you pull.");
  const up = (await upstreamOf(repo, branch)) ?? { remote: "origin", branch };
  const url = await remoteUrl(repo, up.remote);
  const a = auth(net, url);
  try {
    await git.fetch({
      ...base(repo),
      http: net.http as unknown as IsoHttpClient,
      remote: up.remote,
      ref: up.branch,
      singleBranch: true,
      tags: false,
      onAuth: a.onAuth,
      onAuthFailure: a.onAuthFailure,
      onProgress: onProgress(net),
    });
  } catch (e) {
    throw gitError(e, a.host);
  }
  if (!(await upstreamOf(repo, branch))) {
    await git.setConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${branch}.remote`, value: up.remote });
    await git.setConfig({ fs: repo.fs, dir: repo.dir, path: `branch.${branch}.merge`, value: `refs/heads/${up.branch}` });
  }
  return mergeUpstream(repo, identity);
}

/** The local half of `pull`: merges the (fetched) upstream into the current branch, on the
 *  model where a merger is registered (`../merges.ts`) or given. */
export async function mergeUpstream(
  repo: Repo,
  identity: GitIdentity | null,
  merger: CollectionMerger | null = collectionMerger(),
): Promise<GitPullResult> {
  await requireNoMerge(repo, "pull");
  const branch = await git.currentBranch({ fs: repo.fs, dir: repo.dir, fullname: false });
  if (!branch) throw gitErr("Switch to a branch before you pull.");
  const up = await upstreamOf(repo, branch);
  if (!up) throw gitErr(`The branch ${branch} has no upstream.`);
  const theirsRef = `refs/remotes/${up.remote}/${up.branch}`;
  const theirs = await refOid(repo, theirsRef);
  if (!theirs) throw gitErr(`The remote has no branch ${up.branch}.`);
  const ours = await headOid(repo);
  if (ours === theirs) return { outcome: "up-to-date" };
  if (ours && (await git.isDescendent({ ...base(repo), oid: ours, ancestor: theirs, depth: -1 }))) {
    return { outcome: "up-to-date" };
  }
  const oursTree = await flatTree(repo, ours);
  const theirsTree = await flatTree(repo, theirs);
  try {
    if (!ours || (await git.isDescendent({ ...base(repo), oid: theirs, ancestor: ours, depth: -1 }))) {
      await guardLocalChanges(repo, changedPaths(oursTree, theirsTree));
      const commits = await countOnly(repo, theirs, ours);
      await git.writeRef({ fs: repo.fs, dir: repo.dir, ref: `refs/heads/${branch}`, value: theirs, force: true });
      await git.checkout({ ...base(repo), ref: branch });
      return { outcome: "fast-forward", commits };
    }
    if (!identity) throw gitErr("Set your name and email for commits first: this pull creates a merge commit.");
    const [mergeBase] = await git.findMergeBase({ ...base(repo), oids: [ours, theirs] });
    const baseTree = await flatTree(repo, (mergeBase as string) ?? null);
    await guardLocalChanges(repo, changedPaths(baseTree, theirsTree));
    const author = { name: identity.name, email: identity.email };
    const message = `Merge ${up.remote}/${up.branch} into ${branch}`;
    const sem = await semanticMerge(repo, merger, (mergeBase as string) ?? null, ours, theirs);
    try {
      const result = await git.merge({
        ...base(repo),
        ours: branch,
        theirs: theirsRef,
        author,
        committer: author,
        message,
        abortOnConflict: false,
        mergeDriver: merger && sem ? semanticDriver(merger, sem) : undefined,
      });
      await git.checkout({ ...base(repo), ref: branch });
      // The checkout leaves files both sides changed as ours; write the merged ones.
      await syncWorkTree(repo, oursTree, await flatTree(repo, result.oid!));
      return { outcome: "merged", oid: result.oid! };
    } catch (e) {
      const err = e as { code?: string; data?: { filepaths?: string[] } };
      if (err.code !== "MergeConflictError") throw e;
      await writeWorkFile(repo, ".git/MERGE_HEAD", `${theirs}\n`);
      await writeWorkFile(repo, ".git/MERGE_MSG", `${message}\n`);
      if (sem) {
        await applyClean(repo, sem);
        if ((await conflicted(repo)).length === 0) {
          const covered = new Set(sem.paths);
          const others = [...changedPaths(oursTree, theirsTree)].filter((p) => !covered.has(p));
          await takeTheirsOnly(repo, others, baseTree, oursTree, theirsTree);
          const c = await commitPaths(repo, message, [...others, ...sem.paths], identity);
          return { outcome: "merged", oid: c.oid };
        }
      }
      return { outcome: "conflicts", paths: (await conflicted(repo)).map((p) => collectionPath(repo, p)) };
    }
  } catch (e) {
    throw gitError(e);
  }
}

/** The core's merge of the collection: the paths it covers and, to find a file again in
 *  isomorphic-git's merge driver (which only gets the file name and the three texts), the
 *  path of each name with ours' and theirs' text. `null` without a merger or when the core
 *  cannot merge; then isomorphic-git's own merge is all there is. */
interface Semantic extends CollectionMerge {
  paths: string[];
  byContent: Map<string, string>;
}

const contentKey = (name: string, ours: string, theirs: string) => `${name}\u0000${ours}\u0000${theirs}`;
const fileName = (p: string) => p.slice(p.lastIndexOf("/") + 1);

async function semanticMerge(
  repo: Repo,
  merger: CollectionMerger | null,
  mergeBase: string | null,
  ours: string,
  theirs: string,
): Promise<Semantic | null> {
  if (!merger) return null;
  const files = async (oid: string | null): Promise<TextFiles> =>
    oid ? Object.entries((await treeAt(repo, oid)).files) : [];
  try {
    const [b, o, t] = [await files(mergeBase), await files(ours), await files(theirs)];
    const m = await merger.merge(b, o, t);
    // The core works on collection paths; the merge here on repository paths.
    const at = (p: string) => repoPath(repo, p);
    const moved = (list: TextFiles): TextFiles => list.map(([p, text]) => [at(p), text]);
    const merged: CollectionMerge = {
      files: moved(m.files),
      conflicts: m.conflicts.map((c) => ({ ...c, file: at(c.file) })),
      text_conflicts: m.text_conflicts.map(at),
      marked: moved(m.marked),
    };
    const paths = [...new Set([...b, ...o, ...t].map(([p]) => at(p)))].sort();
    const theirsOf = new Map(t);
    const byContent = new Map<string, string>();
    for (const [p, text] of o) {
      const other = theirsOf.get(p);
      if (other !== undefined) byContent.set(contentKey(fileName(p), text, other), at(p));
    }
    return { ...merged, paths, byContent };
  } catch {
    return null;
  }
}

const conflictedIn = (m: CollectionMerge) => new Set([...m.conflicts.map((c) => c.file), ...m.text_conflicts]);

/** isomorphic-git's merge driver on the core's merge: collection files from `sem`, any other
 *  file both sides changed through the core's line merge. */
function semanticDriver(merger: CollectionMerger, sem: Semantic) {
  const conflicts = conflictedIn(sem);
  const merged = new Map(sem.files);
  const marked = new Map(sem.marked);
  return async ({ path, contents }: { branches: string[]; contents: string[]; path: string }) => {
    const [b, o, t] = contents;
    const full = sem.byContent.get(contentKey(path, o, t));
    if (full !== undefined) {
      if (conflicts.has(full)) return { cleanMerge: false, mergedText: marked.get(full) ?? o };
      return { cleanMerge: true, mergedText: merged.get(full) ?? o };
    }
    const line = await merger.text(b === "" ? null : b, o, t);
    return { cleanMerge: line.clean, mergedText: line.text };
  };
}

/** Writes the files that differ between `from` (the work tree's commit) and `to` into the
 *  work tree and the index. */
async function syncWorkTree(repo: Repo, from: Map<string, Entry>, to: Map<string, Entry>): Promise<void> {
  for (const p of changedPaths(from, to)) {
    const e = to.get(p);
    if (!e) {
      await removeOptional(repo, p);
      await git.remove({ fs: repo.fs, dir: repo.dir, filepath: p });
    } else {
      const { blob } = await git.readBlob({ ...base(repo), oid: e.oid });
      const have = await readOptional(repo, p);
      if (have && have.length === blob.length && have.every((b, i) => b === blob[i])) continue;
      await writeWorkFile(repo, p, blob);
      await git.add({ ...base(repo), filepath: p });
    }
  }
}

/** After a stopped merge: writes the collection files the core merged cleanly (e.g. a request
 *  one side renamed and the other changed) and marks them resolved. */
async function applyClean(repo: Repo, sem: Semantic): Promise<void> {
  const conflicts = conflictedIn(sem);
  const merged = new Map(sem.files);
  const unmerged = new Set(await conflicted(repo));
  for (const p of sem.paths) {
    if (conflicts.has(p)) continue;
    const want = merged.get(p) ?? null;
    const have = text(await readOptional(repo, p));
    if (want === have && !unmerged.has(p)) continue;
    if (want === null) {
      await removeOptional(repo, p);
      await git.remove({ fs: repo.fs, dir: repo.dir, filepath: p });
    } else {
      await writeWorkFile(repo, p, want);
      await git.add({ ...base(repo), filepath: p });
    }
  }
}

/** The files outside the collection after a stopped merge that the core resolved: a file
 *  only theirs changed gets theirs' version (isomorphic-git leaves a file theirs deleted). */
async function takeTheirsOnly(
  repo: Repo,
  paths: Iterable<string>,
  bases: Map<string, Entry>,
  ours: Map<string, Entry>,
  theirs: Map<string, Entry>,
): Promise<void> {
  for (const p of paths) {
    if (ours.get(p)?.oid !== bases.get(p)?.oid) continue;
    const t = theirs.get(p);
    if (!t) {
      await removeOptional(repo, p);
    } else {
      const { blob } = await git.readBlob({ ...base(repo), oid: t.oid });
      await writeWorkFile(repo, p, blob);
    }
  }
}

/** The merge in progress for `MergeApi`: whether one is, the collection at HEAD and the
 *  conflicted files' base, ours and theirs (from the merge base, HEAD and MERGE_HEAD). */
export async function mergeSides(
  repo: Repo,
): Promise<{ merging: boolean; context: TextFiles; stages: ConflictStages[] }> {
  const merging = (await mergeHead(repo)) !== null;
  const paths = (await conflicted(repo)).map((p) => collectionPath(repo, p));
  if (!merging || paths.length === 0) return { merging, context: [], stages: [] };
  const context = Object.entries((await treeAt(repo, "HEAD")).files);
  const stages: ConflictStages[] = [];
  for (const p of paths) {
    const c = await conflict(repo, p);
    stages.push({ path: p, base: c.base, ours: c.ours, theirs: c.theirs });
  }
  return { merging, context, stages };
}

/** Writes resolved files (`null`: delete) and marks them resolved in the index. */
export async function writeResolved(repo: Repo, files: [string, string | null][]): Promise<void> {
  for (const [rel, content] of files) {
    const p = repoPath(repo, rel);
    if (content === null) {
      await removeOptional(repo, p);
      await git.remove({ fs: repo.fs, dir: repo.dir, filepath: p });
    } else {
      await writeWorkFile(repo, p, content);
      await git.add({ ...base(repo), filepath: p });
    }
  }
}

const text = (b: Uint8Array | null) => (b ? new TextDecoder().decode(b) : null);

export async function conflict(repo: Repo, path: string): Promise<GitConflict> {
  const filepath = repoPath(repo, path);
  const theirs = await mergeHead(repo);
  if (!theirs) throw gitErr("No merge is in progress.");
  const ours = await headOid(repo);
  const bases = ours ? ((await git.findMergeBase({ ...base(repo), oids: [ours, theirs] })) as string[]) : [];
  return {
    path,
    base: text(await blobAt(repo, bases[0] ?? null, filepath)),
    ours: text(await blobAt(repo, ours, filepath)),
    theirs: text(await blobAt(repo, theirs, filepath)),
  };
}

export async function resolve(repo: Repo, path: string, resolution: GitResolution): Promise<void> {
  const filepath = repoPath(repo, path);
  const theirs = await mergeHead(repo);
  if (!theirs) throw gitErr("No merge is in progress.");
  const side = resolution === "ours" ? await headOid(repo) : theirs;
  const bytes = await blobAt(repo, side, filepath);
  if (bytes) {
    await writeWorkFile(repo, filepath, bytes);
    await git.add({ ...base(repo), filepath });
  } else {
    await removeOptional(repo, filepath);
    await git.remove({ fs: repo.fs, dir: repo.dir, filepath });
  }
}

export async function clone(repo: Repo, net: Network, url: string): Promise<void> {
  const a = auth(net, url);
  try {
    await git.clone({
      ...base(repo),
      http: net.http as unknown as IsoHttpClient,
      url,
      onAuth: a.onAuth,
      onAuthFailure: a.onAuthFailure,
      onProgress: onProgress(net),
    });
  } catch (e) {
    throw gitError(e, a.host);
  }
}

/** Folder name for a clone of `url`: its last path segment without `.git`. */
export function cloneFolderName(url: string): string {
  const trimmed = url.trim().replace(/[?#].*$/, "").replace(/\/+$/, "");
  const last = trimmed.split(/[/:]/).pop() ?? "";
  const name = last.replace(/\.git$/, "").replace(/[^A-Za-z0-9._-]+/g, "-");
  return name && name !== "." && name !== ".." ? name : "repository";
}
