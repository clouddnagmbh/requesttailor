// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: the Git wizards' backend of the web app (`GitApi.plan`, `apply`, `undo`,
// `lastAction`, `remotes`, `stashes`) on isomorphic-git, with the rules every shell follows
// (`rt_app::git::actions`; the mock in apps/ui/src/commands/mock-git-actions.ts has the same
// wording):
//
// - The repository comes only from `ctx.repo()`; every path is relative to its root (a
//   collection may sit in a subfolder). Paths in plans and stash lists carry a leading `/`.
// - `plan` only reads (no refs, index, working files or config change); `apply` plans again
//   and refuses with kind `git` and the plan's message while it is blocked.
// - Whatever moves a ref or changes files writes backup refs
//   `refs/requesttailor/backup/<unix>/<name>` first and, unless it cannot be undone (push,
//   publish, push-tag, fetch), an entry in `.git/requesttailor/actions.jsonl` (newest last, at
//   most 20). Working files an action replaces are kept in a snapshot commit in Git's stash
//   format (so `git stash apply <backup ref>` brings them back on the command line too).
// - `undo` refuses once a recorded ref moved since; otherwise it puts refs, `HEAD`, config,
//   the stash list and the files back and removes the entry. A journal line keeps what the
//   web shell needs beyond `GitUndoEntry` in a `web` field (serde ignores unknown fields).
// - "Park changes" is a real Git stash: `refs/stash` with its reflog `.git/logs/refs/stash`, a
//   stash commit with the parents `HEAD`, an index commit and (for untracked files) a third,
//   so `git stash list` and `git stash apply` see the same.
// - Merge, cherry-pick and revert merge three ways on the trees: the files of each collection
//   (a folder with `opencollection.yml`) through the core's semantic merge
//   (`collectionMerger()` of ../merges.ts, as the pull in ./repo.ts), other files through the
//   core's line merge (whole files without a merger). A merge that stops leaves Git's state:
//   `MERGE_HEAD` (`CHERRY_PICK_HEAD`/`REVERT_HEAD` for a pick or revert), conflict stages in
//   the index and the marked files, as the pull does.
// - Network (fetch, push, publish, push-tag, pull) through the companion (`ctx.network()`);
//   errors map with `gitError`, so missing credentials reach the wizard as `git-auth`.
// - No force push, no rebase.
//
// isomorphic-git loads on the first call (as everywhere in ./index.ts), not with this module.

import {
  BackendError,
  type GitAction,
  type GitActionMethods,
  type GitActionResult,
  type GitBlock,
  type GitBlockReason,
  type GitCommit,
  type GitFile,
  type GitIdentity,
  type GitMergeFile,
  type GitPlan,
  type GitPullResult,
  type GitRefMove,
  type GitRemote,
  type GitStash,
  type GitUndoEntry,
  refNameProblem,
} from "@requesttailor/ui";

import { collectionMerger, type CollectionMerge, type TextFiles } from "../merges";
import type { WebGitContext } from "./graph";
import { hostOf } from "./http";
import type { Network, Repo } from "./repo";
import { collectionPath, repoPath } from "./scope";

/** What the wizards need beyond the graph's context: the network and the author identity
 *  (./index.ts passes the same as for `push`, `pull` and `commit`). */
export interface WebGitActionContext extends WebGitContext {
  network(): Promise<Network>;
  identity(): GitIdentity | null;
}

// ---------------------------------------------------------------- loading

type Iso = typeof import("isomorphic-git").default;
type RepoOps = typeof import("./repo");
type Revisions = typeof import("./revisions");
type WalkerEntry = import("isomorphic-git").WalkerEntry;

let git: Iso;
let ops: RepoOps;
let revs: Revisions;
let loading: Promise<void> | null = null;

function ready(): Promise<void> {
  loading ??= (async () => {
    // ./repo first: it installs the Buffer polyfill isomorphic-git needs in the browser.
    ops = await import("./repo");
    revs = await import("./revisions");
    const m = (await import("isomorphic-git")) as unknown as { default?: Iso };
    git = m.default ?? (m as unknown as Iso);
  })().catch((e: unknown) => {
    loading = null;
    throw e;
  });
  return loading;
}

// ---------------------------------------------------------------- constants and words

/** Entries kept in the action journal (`rt_app::git::actions::UNDO_KEEP`). */
export const UNDO_KEEP = 20;
/** Prefix of the backup refs (`rt_app::rescue::BACKUP_PREFIX`). */
export const BACKUP_PREFIX = "refs/requesttailor/backup/";
export const JOURNAL = ".git/requesttailor/actions.jsonl";
const STASH_REF = "refs/stash";
const STASH_LOG = ".git/logs/refs/stash";
const COLLECTION_FILE = "opencollection.yml";
const ZERO = "0".repeat(40);
const COUNT_LIMIT = 10_000;
/** Author of stash and snapshot commits when no identity is set (they stay local). */
const FALLBACK_WHO: GitIdentity = { name: "RequestTailor", email: "requesttailor@localhost" };
const IN_PROGRESS_FILES = [".git/MERGE_HEAD", ".git/CHERRY_PICK_HEAD", ".git/REVERT_HEAD"];

const short = (oid: string) => oid.slice(0, 7);
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const slash = (p: string) => (p.startsWith("/") ? p : `/${p}`);
const unslash = (p: string) => p.replace(/^\/+/, "");
const subjectOf = (message: string) => message.split("\n")[0];
const branchRef = (name: string) => `refs/heads/${name}`;
const gitErr = (message: string) => new BackendError("git", message);
const sorted = (paths: Iterable<string>) => [...new Set(paths)].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));

/** The backup ref for `name` at `unix` seconds (`rt_app::git::actions::backup_ref`). */
export function backupRef(unix: number, name: string): string {
  return `${BACKUP_PREFIX}${unix}/${name}`;
}

// ---------------------------------------------------------------- low-level helpers

interface Entry {
  mode: string;
  oid: string;
  type: "blob" | "tree" | "commit";
}
type Flat = Map<string, Entry>;
type Content = { entry: Entry } | { text: string };

function abs(r: Repo, rel: string): string {
  return `${r.dir.replace(/\/+$/, "")}/${rel}`;
}

const b = (r: Repo) => ({ fs: r.fs, dir: r.dir, cache: r.cache });
const code = (e: unknown) => (e as { code?: string }).code;
const decoder = new TextDecoder("utf-8", { fatal: true });

function utf8(bytes: Uint8Array): string | null {
  try {
    return decoder.decode(bytes);
  } catch {
    return null;
  }
}

async function readOptional(r: Repo, rel: string): Promise<Uint8Array | null> {
  try {
    return (await r.fs.promises.readFile(abs(r, rel))) as Uint8Array;
  } catch (e) {
    const c = code(e);
    if (c === "ENOENT" || c === "ENOTDIR" || c === "EISDIR") return null;
    throw e;
  }
}

async function readText(r: Repo, rel: string): Promise<string | null> {
  const bytes = await readOptional(r, rel);
  return bytes ? new TextDecoder().decode(bytes) : null;
}

async function writeWorkFile(r: Repo, rel: string, data: Uint8Array | string): Promise<void> {
  let dir = r.dir.replace(/\/+$/, "");
  for (const part of rel.split("/").slice(0, -1)) {
    dir = `${dir}/${part}`;
    try {
      await r.fs.promises.mkdir(dir);
    } catch (e) {
      if (code(e) !== "EEXIST") throw e;
    }
  }
  await r.fs.promises.writeFile(abs(r, rel), data);
}

/** Removes a file and the folders it leaves empty (not the root). */
async function removeWorkFile(r: Repo, rel: string): Promise<void> {
  try {
    await r.fs.promises.unlink(abs(r, rel));
  } catch (e) {
    if (code(e) !== "ENOENT" && code(e) !== "ENOTDIR") throw e;
    return;
  }
  const parts = rel.split("/").slice(0, -1);
  while (parts.length > 0 && parts[0] !== ".git") {
    const dir = abs(r, parts.join("/"));
    try {
      if ((await r.fs.promises.readdir(dir)).length > 0) return;
      await r.fs.promises.rmdir(dir);
    } catch {
      return;
    }
    parts.pop();
  }
}

async function refOid(r: Repo, ref: string): Promise<string | null> {
  try {
    return await git.resolveRef({ fs: r.fs, dir: r.dir, ref });
  } catch {
    return null;
  }
}

const headOid = (r: Repo) => refOid(r, "HEAD");

/** The value of a ref by full name, as the journal records it (`HEAD`: its commit). */
const refValue = (r: Repo, name: string) => refOid(r, name);

async function currentBranch(r: Repo): Promise<string | null> {
  return (await git.currentBranch({ fs: r.fs, dir: r.dir, fullname: false })) ?? null;
}

/** `HEAD` as written: `ref: refs/heads/main` or a commit id. */
async function rawHead(r: Repo): Promise<string> {
  return ((await readText(r, ".git/HEAD")) ?? "").trim();
}

async function writeHead(r: Repo, raw: string): Promise<void> {
  if (raw.startsWith("ref: ")) {
    await git.writeRef({ fs: r.fs, dir: r.dir, ref: "HEAD", value: raw.slice(5).trim(), symbolic: true, force: true });
  } else if (/^[0-9a-f]{40}$/.test(raw)) {
    await git.writeRef({ fs: r.fs, dir: r.dir, ref: "HEAD", value: raw, force: true });
  }
}

async function deleteRefIfAny(r: Repo, ref: string): Promise<void> {
  if ((await refOid(r, ref)) === null) return;
  await git.deleteRef({ fs: r.fs, dir: r.dir, ref });
}

async function setRef(r: Repo, ref: string, oid: string | null): Promise<void> {
  if (oid === null) await deleteRefIfAny(r, ref);
  else await git.writeRef({ fs: r.fs, dir: r.dir, ref, value: oid, force: true });
}

async function readCommit(r: Repo, oid: string) {
  return (await git.readCommit({ ...b(r), oid })).commit;
}

function toCommit(oid: string, c: { message: string; author: { name: string; timestamp: number } }): GitCommit {
  return { oid, summary: subjectOf(c.message), author: c.author.name, time: c.author.timestamp };
}

async function flatOfTree(r: Repo, tree: string): Promise<Flat> {
  const out: Flat = new Map();
  const walk = async (oid: string, prefix: string) => {
    for (const e of (await git.readTree({ ...b(r), oid })).tree) {
      const p = prefix ? `${prefix}/${e.path}` : e.path;
      if (e.type === "tree") await walk(e.oid, p);
      else out.set(p, { mode: e.mode, oid: e.oid, type: e.type });
    }
  };
  await walk(tree, "");
  return out;
}

/** Flat map path → entry of a commit's tree; empty for `null`. */
async function flatTree(r: Repo, commit: string | null): Promise<Flat> {
  if (!commit) return new Map();
  return flatOfTree(r, (await readCommit(r, commit)).tree);
}

async function writeFlatTree(r: Repo, entries: Flat): Promise<string> {
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
    return git.writeTree({ fs: r.fs, dir: r.dir, tree });
  };
  return write(root);
}

/** Paths whose content differs between two flat trees. */
function changedPaths(a: Flat, z: Flat): Set<string> {
  const out = new Set<string>();
  for (const [p, e] of a) if (z.get(p)?.oid !== e.oid) out.add(p);
  for (const p of z.keys()) if (!a.has(p)) out.add(p);
  return out;
}

const sameEntry = (x: Entry | undefined, y: Entry | undefined) => (x?.oid ?? null) === (y?.oid ?? null);

async function blobText(r: Repo, e: Entry | undefined | null): Promise<string | null> {
  if (!e) return null;
  return utf8((await git.readBlob({ ...b(r), oid: e.oid })).blob);
}

/** Writes `content` (or removes the file for `null`) in the working tree. */
async function writeContent(r: Repo, p: string, content: Content | null): Promise<void> {
  if (content === null) await removeWorkFile(r, p);
  else if ("text" in content) await writeWorkFile(r, p, content.text);
  else await writeWorkFile(r, p, (await git.readBlob({ ...b(r), oid: content.entry.oid })).blob);
}

const entryContent = (e: Entry | undefined | null): Content | null => (e ? { entry: e } : null);

/** The index as a flat map (stage 0). */
async function indexFlat(r: Repo): Promise<Flat> {
  const out: Flat = new Map();
  await git.walk({
    ...b(r),
    trees: [git.STAGE()],
    map: async (filepath: string, [e]: (WalkerEntry | null)[]) => {
      if (!e || filepath === ".") return true;
      if ((await e.type()) === "tree") return true;
      const mode = (await e.mode()).toString(8).padStart(6, "0");
      out.set(filepath, { mode, oid: await e.oid(), type: mode === "160000" ? "commit" : "blob" });
      return true;
    },
  });
  return out;
}

/** Makes the index entries of `paths` match `head` (removed where `head` has none). */
async function indexToHead(r: Repo, paths: Iterable<string>, head: string | null): Promise<void> {
  for (const p of new Set(paths)) {
    if (head) await git.resetIndex({ fs: r.fs, dir: r.dir, filepath: p, ref: head });
    else await git.remove({ fs: r.fs, dir: r.dir, filepath: p });
  }
}

/** Moves working files and index from tree `from` to tree `to` (only the paths that differ). */
async function moveWork(r: Repo, from: Flat, to: Flat): Promise<string[]> {
  const paths = sorted(changedPaths(from, to));
  for (const p of paths) {
    const e = to.get(p);
    await writeContent(r, p, entryContent(e));
    if (e) await git.add({ ...b(r), filepath: p });
    else await git.remove({ fs: r.fs, dir: r.dir, filepath: p });
  }
  return paths;
}

async function conflictedPaths(r: Repo): Promise<string[]> {
  const index = await readOptional(r, ".git/index");
  return index ? ops.unmergedPaths(index) : [];
}

/** The working tree's changes, as `GitApi.status` lists them, without touching the index. */
async function workFiles(r: Repo): Promise<GitFile[]> {
  const unmerged = new Set(await conflictedPaths(r));
  const files: GitFile[] = [];
  for (const [p, h, w] of await git.statusMatrix({ ...b(r), refresh: false })) {
    if (unmerged.has(p)) files.push({ path: p, state: "conflicted" });
    else if (h === 0 && w !== 0) files.push({ path: p, state: "untracked" });
    else if (h === 1 && w === 0) files.push({ path: p, state: "deleted" });
    else if (h === 1 && w === 2) files.push({ path: p, state: "modified" });
  }
  for (const p of unmerged) if (!files.some((f) => f.path === p)) files.push({ path: p, state: "conflicted" });
  return files.sort((x, y) => (x.path < y.path ? -1 : x.path > y.path ? 1 : 0));
}

async function isAncestor(r: Repo, ancestor: string, of: string): Promise<boolean> {
  if (ancestor === of) return true;
  return git.isDescendent({ ...b(r), oid: of, ancestor, depth: -1 });
}

async function ancestors(r: Repo, tips: (string | null)[]): Promise<Set<string>> {
  const seen = new Set<string>();
  const todo = tips.filter((t): t is string => !!t);
  while (todo.length > 0 && seen.size < COUNT_LIMIT) {
    const oid = todo.pop()!;
    if (seen.has(oid)) continue;
    seen.add(oid);
    try {
      todo.push(...(await readCommit(r, oid)).parent);
    } catch {
      // Missing (shallow) history: stop here.
    }
  }
  return seen;
}

/** Commits reachable from `from` but not from any of `not`, newest first. */
async function commitsOnly(r: Repo, from: string, not: (string | null)[], limit = COUNT_LIMIT): Promise<GitCommit[]> {
  const hidden = await ancestors(r, not);
  const out: { commit: GitCommit; time: number }[] = [];
  const seen = new Set<string>();
  const todo = [from];
  while (todo.length > 0 && seen.size < COUNT_LIMIT) {
    const oid = todo.pop()!;
    if (seen.has(oid) || hidden.has(oid)) continue;
    seen.add(oid);
    const c = await readCommit(r, oid);
    out.push({ commit: toCommit(oid, c), time: c.committer.timestamp });
    todo.push(...c.parent);
  }
  return out
    .sort((x, y) => y.time - x.time)
    .slice(0, limit)
    .map((x) => x.commit);
}

async function peel(r: Repo, oid: string): Promise<string> {
  const { type } = await git.readObject({ ...b(r), oid });
  return type === "tag" ? (await git.readTag({ ...b(r), oid })).tag.object : oid;
}

interface Upstream {
  remote: string;
  branch: string;
}

async function getConfig(r: Repo, path: string): Promise<string | undefined> {
  const v: unknown = await git.getConfig({ fs: r.fs, dir: r.dir, path });
  return v === undefined || v === null ? undefined : String(v);
}

async function upstreamOf(r: Repo, branch: string): Promise<Upstream | null> {
  const remote = await getConfig(r, `branch.${branch}.remote`);
  const merge = await getConfig(r, `branch.${branch}.merge`);
  if (!remote || !merge) return null;
  return { remote, branch: merge.replace(/^refs\/heads\//, "") };
}

async function remoteNames(r: Repo): Promise<string[]> {
  return (await git.listRemotes({ fs: r.fs, dir: r.dir })).map((x) => x.remote);
}

const remoteUrl = (r: Repo, name: string) => getConfig(r, `remote.${name}.url`);

/** Remote-tracking refs of `remote` (full names, without the symbolic `HEAD`). */
async function remoteRefs(r: Repo, remote: string): Promise<string[]> {
  const names = await git.listBranches({ fs: r.fs, dir: r.dir, remote }).catch(() => [] as string[]);
  return names.filter((n) => n !== "HEAD").map((n) => `refs/remotes/${remote}/${n}`);
}

/** `https://user:secret@host/x` → `https://host/x`. */
const withoutCredentials = (url: string) => url.replace(/^([a-z][a-z0-9+.-]*:\/\/)[^@/]*@/i, "$1");

// ---------------------------------------------------------------- collections in the tree

/** The collection folders of the trees (`""` for the root, else `api/`), deepest first: a
 *  folder with `opencollection.yml` outside hidden folders and `node_modules`. */
function collectionRoots(flats: Flat[]): string[] {
  const roots = new Set<string>();
  for (const f of flats) {
    for (const p of f.keys()) {
      if (p === COLLECTION_FILE) roots.add("");
      else if (p.endsWith(`/${COLLECTION_FILE}`)) {
        const dir = p.slice(0, -COLLECTION_FILE.length);
        if (dir.split("/").slice(0, -1).every((s) => !s.startsWith(".") && s !== "node_modules")) roots.add(dir);
      }
    }
  }
  return [...roots].sort((x, y) => y.length - x.length);
}

/** The collection folder `p` belongs to as a collection file, or `null`. */
function collectionOf(p: string, roots: string[]): string | null {
  for (const dir of roots) {
    if (!p.startsWith(dir)) continue;
    return revs.treePath(p.slice(dir.length)) ? dir : null;
  }
  return null;
}

// ---------------------------------------------------------------- config sections (for undo)

const SECTION_KEYS = {
  remote: ["url", "pushurl", "fetch", "push", "tagopt", "prune", "mirror", "proxy"],
  branch: ["remote", "merge", "rebase", "pushremote", "description"],
} as const;

interface ConfigSection {
  section: "remote" | "branch";
  name: string;
  /** `null`: the section had no values. */
  values: Record<string, string[]> | null;
}

async function configAll(r: Repo, path: string): Promise<string[]> {
  return ((await git.getConfigAll({ fs: r.fs, dir: r.dir, path })) as unknown[]).map(String);
}

async function readSection(r: Repo, section: "remote" | "branch", name: string): Promise<ConfigSection> {
  const values: Record<string, string[]> = {};
  for (const key of SECTION_KEYS[section]) {
    const all = await configAll(r, `${section}.${name}.${key}`);
    if (all.length > 0) values[key] = all;
  }
  return { section, name, values: Object.keys(values).length > 0 ? values : null };
}

async function clearSection(r: Repo, section: "remote" | "branch", name: string): Promise<void> {
  if (section === "remote" && (await remoteNames(r)).includes(name)) {
    await git.deleteRemote({ fs: r.fs, dir: r.dir, remote: name });
  }
  for (const key of SECTION_KEYS[section]) {
    const path = `${section}.${name}.${key}`;
    for (let i = 0; i < 64 && (await configAll(r, path)).length > 0; i++) {
      await git.setConfig({ fs: r.fs, dir: r.dir, path, value: undefined });
    }
  }
}

async function writeSection(r: Repo, s: ConfigSection): Promise<void> {
  await clearSection(r, s.section, s.name);
  for (const [key, values] of Object.entries(s.values ?? {})) {
    for (const [i, value] of values.entries()) {
      await git.setConfig({ fs: r.fs, dir: r.dir, path: `${s.section}.${s.name}.${key}`, value, append: i > 0 });
    }
  }
}

/** Local branches whose upstream is on `remote`. */
async function trackingBranches(r: Repo, remote: string): Promise<string[]> {
  const out: string[] = [];
  for (const name of await git.listBranches({ fs: r.fs, dir: r.dir })) {
    if ((await getConfig(r, `branch.${name}.remote`)) === remote) out.push(name);
  }
  return out;
}

// ---------------------------------------------------------------- stash format

interface StashLine {
  oid: string;
  /** `Name <email> <unix> <tz>\t<message>` as in the reflog. */
  rest: string;
  time: number;
  message: string;
}

function parseReflog(text: string): StashLine[] {
  const out: StashLine[] = [];
  for (const line of text.split("\n")) {
    const m = /^[0-9a-f]{40} ([0-9a-f]{40}) (.*)$/.exec(line);
    if (!m) continue;
    const tab = m[2].indexOf("\t");
    const who = tab >= 0 ? m[2].slice(0, tab) : m[2];
    const time = Number(/> (\d+) [+-]\d{4}$/.exec(who)?.[1] ?? 0);
    out.push({ oid: m[1], rest: m[2], time, message: tab >= 0 ? m[2].slice(tab + 1) : "" });
  }
  return out;
}

function tzOffset(t: number): string {
  const east = -new Date(t * 1000).getTimezoneOffset();
  const a = Math.abs(east);
  return `${east >= 0 ? "+" : "-"}${String(Math.floor(a / 60)).padStart(2, "0")}${String(a % 60).padStart(2, "0")}`;
}

function signature(who: GitIdentity, t: number) {
  return { name: who.name, email: who.email, timestamp: t, timezoneOffset: new Date(t * 1000).getTimezoneOffset() };
}

const reflogRest = (who: GitIdentity, t: number, message: string) => `${who.name} <${who.email}> ${t} ${tzOffset(t)}\t${message}`;

/** The stash list, newest first (`stash@{0}` first). */
async function stashLines(r: Repo): Promise<StashLine[]> {
  const lines = parseReflog((await readText(r, STASH_LOG)) ?? "");
  if (lines.length > 0) return lines.reverse();
  const top = await refOid(r, STASH_REF);
  if (!top) return [];
  const c = await readCommit(r, top);
  const message = subjectOf(c.message);
  return [{ oid: top, rest: reflogRest(c.committer, c.committer.timestamp, message), time: c.committer.timestamp, message }];
}

/** Writes the stash list (newest first) as `refs/stash` and its reflog, as `git stash drop`. */
async function writeStashLines(r: Repo, newestFirst: StashLine[]): Promise<void> {
  const lines = [...newestFirst].reverse();
  if (lines.length === 0) {
    await deleteRefIfAny(r, STASH_REF);
    await removeWorkFile(r, STASH_LOG);
    return;
  }
  let old = ZERO;
  const text = lines
    .map((l) => {
      const line = `${old} ${l.oid} ${l.rest}\n`;
      old = l.oid;
      return line;
    })
    .join("");
  await writeWorkFile(r, STASH_LOG, text);
  await setRef(r, STASH_REF, lines[lines.length - 1].oid);
}

function parseStashMessage(raw: string): { branch?: string; message: string } {
  const m = /^(?:WIP on|On) (.*?): ([\s\S]*)$/.exec(raw.trim());
  if (!m) return { message: raw.trim() };
  return { branch: m[1] === "(no branch)" ? undefined : m[1], message: m[2].trim() };
}

/** What a stash (or snapshot) commit holds: the tree it was made on (`base`) and the files that
 *  differ from it, untracked ones from the third parent included (`null`: deleted). */
async function stashContent(r: Repo, oid: string): Promise<{ base: Flat; files: Map<string, Entry | null> }> {
  const c = await readCommit(r, oid);
  const base = await flatTree(r, c.parent[0] ?? null);
  const work = await flatOfTree(r, c.tree);
  const files = new Map<string, Entry | null>();
  for (const p of changedPaths(base, work)) files.set(p, work.get(p) ?? null);
  if (c.parent[2]) for (const [p, e] of await flatTree(r, c.parent[2])) files.set(p, e);
  return { base, files };
}

/** A commit of the working files in Git's stash format (parents: `HEAD`, the index, and the
 *  untracked files if any); `null` when nothing differs from `HEAD`. Writes objects only. */
async function snapshotCommit(
  r: Repo,
  head: string,
  branch: string | null,
  who: GitIdentity,
  t: number,
  message: string,
): Promise<{ oid: string; files: string[]; message: string } | null> {
  const headFlat = await flatTree(r, head);
  const index = await indexFlat(r);
  const work: Flat = new Map(index);
  const untracked: Flat = new Map();
  const files: string[] = [];
  const blobOf = async (p: string, mode = "100644"): Promise<Entry> => ({
    mode: mode === "100755" ? mode : "100644",
    oid: await git.writeBlob({ fs: r.fs, dir: r.dir, blob: (await readOptional(r, p)) ?? new Uint8Array() }),
    type: "blob",
  });
  for (const [p, h, w, s] of await git.statusMatrix({ ...b(r), refresh: false })) {
    const tracked = h === 1 || s !== 0;
    if (!tracked) {
      if (w === 2) {
        untracked.set(p, await blobOf(p));
        files.push(p);
      }
      continue;
    }
    if (h === 1 && w === 1 && s === 1) continue;
    files.push(p);
    if (w === 0) work.delete(p);
    else if (w === 1 && headFlat.has(p)) work.set(p, headFlat.get(p)!);
    else work.set(p, await blobOf(p, index.get(p)?.mode ?? headFlat.get(p)?.mode));
  }
  if (files.length === 0) return null;
  const label = branch ?? "(no branch)";
  const tail = `${short(head)} ${subjectOf((await readCommit(r, head)).message)}`;
  const sig = signature(who, t);
  const commit = (tree: string, parent: string[], msg: string) =>
    git.writeCommit({ fs: r.fs, dir: r.dir, commit: { tree, parent, message: `${msg}\n`, author: sig, committer: sig } });
  const indexCommit = await commit(await writeFlatTree(r, index), [head], `index on ${label}: ${tail}`);
  const parents = [head, indexCommit];
  if (untracked.size > 0) parents.push(await commit(await writeFlatTree(r, untracked), [], `untracked files on ${label}: ${tail}`));
  const full = `On ${label}: ${message}`;
  const oid = await commit(await writeFlatTree(r, work), parents, full);
  return { oid, files: sorted(files), message: full };
}

// ---------------------------------------------------------------- three-way merge

/** One file of a three-way merge: what goes into the working tree and, when it conflicts,
 *  the stages for the index. */
interface FileMerge {
  path: string;
  semantic: boolean;
  /** The merged file (or the marked one while conflicted); `null`: deleted. */
  content: Content | null;
  /** Field paths (semantic), `lines a-b` (line merge) or a description. */
  conflicts: string[];
  /** Base, ours, theirs while conflicted. */
  stages?: [Entry | undefined, Entry | undefined, Entry | undefined];
}

interface Merged {
  files: FileMerge[];
  ours: Flat;
}

const conflictedIn = (m: CollectionMerge) => new Set([...m.conflicts.map((c) => c.file), ...m.text_conflicts]);

function markerRanges(text: string): string[] {
  const lines = text.split("\n");
  const out: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].startsWith("<<<<<<<")) continue;
    let j = i + 1;
    while (j < lines.length && !lines[j].startsWith(">>>>>>>")) j++;
    out.push(`lines ${i + 1}-${Math.min(j, lines.length - 1) + 1}`);
    i = j;
  }
  return out;
}

interface SemanticResult {
  paths: Set<string>;
  merged: Map<string, string>;
  marked: Map<string, string>;
  conflicted: Set<string>;
  fields: Map<string, string[]>;
}

/** The core's merge of every collection of the trees, paths relative to the repository. */
async function semanticMerge(r: Repo, bf: Flat, of: Flat, tf: Flat): Promise<SemanticResult | null> {
  const merger = collectionMerger();
  if (!merger) return null;
  const roots = collectionRoots([bf, of, tf]);
  const out: SemanticResult = { paths: new Set(), merged: new Map(), marked: new Map(), conflicted: new Set(), fields: new Map() };
  for (const dir of roots) {
    const files = async (f: Flat): Promise<TextFiles> => {
      const list: TextFiles = [];
      for (const [p, e] of f) {
        if (collectionOf(p, roots) !== dir || (e.mode !== "100644" && e.mode !== "100755")) continue;
        const text = await blobText(r, e);
        if (text !== null) list.push([p.slice(dir.length), text]);
      }
      return list;
    };
    const [bt, ot, tt] = [await files(bf), await files(of), await files(tf)];
    let m: CollectionMerge;
    try {
      m = await merger.merge(bt, ot, tt);
    } catch {
      continue; // The core cannot read this collection: its files merge line by line.
    }
    for (const [p] of [...bt, ...ot, ...tt]) out.paths.add(dir + p);
    for (const [p, text] of m.files) out.merged.set(dir + p, text);
    for (const [p, text] of m.marked) out.marked.set(dir + p, text);
    for (const p of conflictedIn(m)) out.conflicted.add(dir + p);
    for (const c of m.conflicts) {
      const key = dir + c.file;
      out.fields.set(key, [...(out.fields.get(key) ?? []), c.field.join(".") || "the whole file"]);
    }
  }
  return out;
}

/** The merge of `theirs` into `ours` over `base`, computed without writing anything. */
async function computeMerge(r: Repo, base: string | null, ours: string, theirs: string): Promise<Merged> {
  const [bf, of, tf] = [await flatTree(r, base), await flatTree(r, ours), await flatTree(r, theirs)];
  const sem = await semanticMerge(r, bf, of, tf);
  const merger = collectionMerger();
  const out: FileMerge[] = [];
  for (const p of sorted([...bf.keys(), ...of.keys(), ...tf.keys()])) {
    const [be, oe, te] = [bf.get(p), of.get(p), tf.get(p)];
    if (sem?.paths.has(p)) {
      const oursText = await blobText(r, oe);
      if (sem.conflicted.has(p)) {
        const marked = sem.marked.get(p) ?? oursText ?? "";
        const fields = sem.fields.get(p) ?? markerRanges(marked);
        out.push({ path: p, semantic: true, content: { text: marked }, conflicts: fields.length ? fields : ["the whole file"], stages: [be, oe, te] });
        continue;
      }
      const want = sem.merged.get(p) ?? null;
      if (want === oursText) continue;
      out.push({ path: p, semantic: true, content: want === null ? null : { text: want }, conflicts: [] });
      continue;
    }
    if (sameEntry(oe, te) || sameEntry(be, te)) continue;
    if (sameEntry(be, oe)) {
      out.push({ path: p, semantic: false, content: entryContent(te), conflicts: [] });
      continue;
    }
    const stages: FileMerge["stages"] = [be, oe, te];
    if (!oe || !te) {
      out.push({ path: p, semantic: false, content: entryContent(oe ?? te), conflicts: [!oe ? "deleted here, changed there" : "changed here, deleted there"], stages });
      continue;
    }
    const [bt, ot, tt] = [await blobText(r, be), await blobText(r, oe), await blobText(r, te)];
    if (merger && ot !== null && tt !== null && (!be || bt !== null)) {
      const line = await merger.text(bt, ot, tt);
      if (line.clean) out.push({ path: p, semantic: false, content: { text: line.text }, conflicts: [] });
      else {
        const ranges = markerRanges(line.text);
        out.push({ path: p, semantic: false, content: { text: line.text }, conflicts: ranges.length ? ranges : ["the whole file"], stages });
      }
      continue;
    }
    out.push({ path: p, semantic: false, content: { entry: oe }, conflicts: [be ? "changed on both sides" : "added on both sides"], stages });
  }
  return { files: out, ours: of };
}

const preview = (files: FileMerge[]): GitMergeFile[] =>
  files.map((f) => ({ path: slash(f.path), semantic: f.semantic, clean: f.conflicts.length ? 0 : 1, conflicts: f.conflicts }));

/** Writes a computed merge: without conflicts the merged tree (returned) with working files
 *  and index to match; with conflicts the marked files and the index stages (`null`). */
async function writeMerge(r: Repo, m: Merged): Promise<string | null> {
  const conflicted = m.files.filter((f) => f.conflicts.length > 0);
  let tree: string | null = null;
  if (conflicted.length === 0) {
    const entries: Flat = new Map(m.ours);
    for (const f of m.files) {
      if (f.content === null) entries.delete(f.path);
      else if ("entry" in f.content) entries.set(f.path, f.content.entry);
      else {
        const oid = await git.writeBlob({ fs: r.fs, dir: r.dir, blob: new TextEncoder().encode(f.content.text) });
        entries.set(f.path, { mode: m.ours.get(f.path)?.mode ?? "100644", oid, type: "blob" });
      }
    }
    tree = await writeFlatTree(r, entries);
  }
  for (const f of m.files) {
    await writeContent(r, f.path, f.content);
    if (f.conflicts.length > 0) continue;
    if (f.content === null) await git.remove({ fs: r.fs, dir: r.dir, filepath: f.path });
    else await git.add({ ...b(r), filepath: f.path });
  }
  if (conflicted.length > 0) await writeStages(r, conflicted);
  return tree;
}

/** Conflict stages (1 base, 2 ours, 3 theirs) in the index, as Git and the pull leave them.
 *  isomorphic-git has no public call for stages; its index manager does it (loaded only here). */
async function writeStages(r: Repo, files: FileMerge[]): Promise<void> {
  const { GitIndexManager } = await import("isomorphic-git/managers");
  const { FileSystem } = await import("isomorphic-git/models");
  await GitIndexManager.acquire({ fs: new FileSystem(r.fs) as never, gitdir: abs(r, ".git"), cache: {} }, async (index) => {
    for (const f of files) {
      const [be, oe, te] = f.stages ?? [];
      index.delete({ filepath: f.path });
      if (be) index.insert({ filepath: f.path, oid: be.oid, stage: 1 } as never);
      if (oe) index.insert({ filepath: f.path, oid: oe.oid, stage: 2 } as never);
      if (te) index.insert({ filepath: f.path, oid: te.oid, stage: 3 } as never);
    }
  });
}

// ---------------------------------------------------------------- journal

/** What undo needs beyond `GitUndoEntry` (the `web` field of a journal line). */
interface Extra {
  /** `HEAD` before, as written (`ref: refs/heads/main` or a commit). */
  head?: string;
  /** Refs that did not move but must keep their values (actions that changed files). */
  guard?: [string, string | null][];
  /** Config sections before. */
  config?: ConfigSection[];
  /** The stash reflog before (`null`: none). */
  stashLog?: string | null;
  /** Snapshot commit (stash format) of the working files before. */
  work?: string;
  /** Working files the action replaced: restored from `work`, else from `HEAD`. */
  paths?: string[];
  /** `HEAD` moved without the working files (reset keeping changes). */
  keepWork?: boolean;
  /** The action left a merge, cherry-pick or revert in progress. */
  clearMerge?: boolean;
}

interface Stored extends GitUndoEntry {
  web?: Extra;
}

async function readJournal(r: Repo): Promise<Stored[]> {
  const text = (await readText(r, JOURNAL)) ?? "";
  const out: Stored[] = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    try {
      const v = JSON.parse(line) as Stored;
      if (typeof v.id === "string" && Array.isArray(v.refs)) out.push(v);
    } catch {
      // A broken line: skip it.
    }
  }
  return out;
}

async function writeJournal(r: Repo, entries: Stored[]): Promise<void> {
  const kept = entries.slice(-UNDO_KEEP);
  if (kept.length === 0) await removeWorkFile(r, JOURNAL);
  else await writeWorkFile(r, JOURNAL, kept.map((e) => JSON.stringify(e)).join("\n") + "\n");
}

function publicEntry(e: Stored): GitUndoEntry {
  const { web, ...entry } = e;
  void web;
  return entry;
}

async function staleness(r: Repo, e: Stored): Promise<string | undefined> {
  const checks: [string, string | null][] = [...e.refs.map((m): [string, string | null] => [m.name, m.after]), ...(e.web?.guard ?? [])];
  const moved: string[] = [];
  for (const [name, after] of checks) if ((await refValue(r, name)) !== after) moved.push(name);
  if (moved.length === 0) return undefined;
  const names = moved.map((n) =>
    n === "HEAD" ? "the current commit" : n === STASH_REF ? "the parked changes" : n.replace(/^refs\/(heads|tags|remotes)\//, ""),
  );
  return `${[...new Set(names)].join(", ")} changed since, so undoing would lose newer work.`;
}

// ---------------------------------------------------------------- network

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
  return net.onProgress ? (p: { phase: string; loaded: number; total?: number }) => net.onProgress!(p.phase, p.loaded, p.total) : undefined;
}

// ---------------------------------------------------------------- the actions

interface Done {
  message: string;
  outcome?: GitPullResult;
  reload: boolean;
}

class Session {
  constructor(
    readonly r: Repo,
    readonly ctx: WebGitActionContext,
  ) {}

  private readonly block = (reason: GitBlockReason, message: string): GitBlock => ({ reason, message });

  unsaved(): GitBlock | null {
    let dirty = 0;
    try {
      dirty = this.ctx.host.session().ws.dirty().length;
    } catch {
      dirty = 0; // No collection open: nothing unsaved.
    }
    return dirty > 0 ? this.block("unsaved-changes", "Save your changes in the app first.") : null;
  }

  async inProgress(): Promise<GitBlock | null> {
    const conflicts = (await conflictedPaths(this.r)).length;
    let pending = false;
    for (const f of IN_PROGRESS_FILES) if (await readOptional(this.r, f)) pending = true;
    if (!pending && conflicts === 0) return null;
    return this.block(
      "in-progress",
      conflicts > 0 ? "A merge is waiting for your decision: resolve the conflicts and commit first." : "Commit to finish the merge in progress first.",
    );
  }

  async localChanges(what: string): Promise<GitBlock | null> {
    const files = await workFiles(this.r);
    return files.length > 0
      ? this.block("local-changes", `You have ${plural(files.length, "uncommitted change")}. Commit or park them before you ${what}.`)
      : null;
  }

  /** Files with uncommitted changes that differ between commits `from` and `to`. */
  async touched(from: string | null, to: string): Promise<string[]> {
    const changed = changedPaths(await flatTree(this.r, from), await flatTree(this.r, to));
    return (await workFiles(this.r)).map((f) => f.path).filter((p) => changed.has(p));
  }

  overwritten(paths: string[]): GitBlock | null {
    return paths.length ? this.block("local-changes", `Your changes to ${paths.join(", ")} would be overwritten. Commit or park them first.`) : null;
  }

  async resolve(rev: string): Promise<string | null> {
    try {
      return await revs.revParse(this.r, rev);
    } catch {
      return null;
    }
  }

  async branchExists(name: string): Promise<boolean> {
    return (await refOid(this.r, branchRef(name))) !== null;
  }

  async subject(oid: string): Promise<string> {
    return subjectOf((await readCommit(this.r, oid)).message);
  }

  identity(): GitIdentity {
    const who = this.ctx.identity();
    if (!who) throw gitErr("Set your name and email for commits first.");
    return who;
  }

  /** Commits only on `oid`: on no other branch, remote branch or tag. */
  async onlyOn(oid: string, except: string): Promise<number> {
    const r = this.r;
    const tips: string[] = [];
    for (const name of await git.listBranches({ fs: r.fs, dir: r.dir })) {
      if (name === except) continue;
      const o = await refOid(r, branchRef(name));
      if (o) tips.push(o);
    }
    for (const remote of await remoteNames(r)) {
      for (const ref of await remoteRefs(r, remote)) {
        const o = await refOid(r, ref);
        if (o) tips.push(o);
      }
    }
    for (const tag of await git.listTags({ fs: r.fs, dir: r.dir })) {
      const o = await refOid(r, `refs/tags/${tag}`);
      if (o) tips.push(await peel(r, o).catch(() => o));
    }
    return (await commitsOnly(r, oid, tips)).length;
  }

  /** The remote of a remote-tracking name like `origin/main`, if that ref exists. */
  async remoteOf(name: string): Promise<string | null> {
    for (const remote of await remoteNames(this.r)) {
      if (name.startsWith(`${remote}/`) && (await refOid(this.r, `refs/remotes/${name}`))) return remote;
    }
    return null;
  }

  // ------------------------------------------------------------ plan

  async plan(action: GitAction): Promise<GitPlan> {
    const r = this.r;
    const block = this.block;
    const p: GitPlan = { action, steps: [], refs: [], network: false, backup: false, undoable: true, destructive: false };
    if (!(await ops.isRepo(r))) return { ...p, undoable: false, blocked: block("not-found", "This folder is not a Git repository yet.") };
    const head = await headOid(r);
    const current = await currentBranch(r);
    const cur = current && head ? current : null;
    const stop = (bl: GitBlock | null) => {
      if (bl && !p.blocked) p.blocked = bl;
    };
    const needBranch = () =>
      stop(cur ? null : block("detached", current ? "Make the first commit first." : "You are not on a branch. Create a branch here or switch to one first."));

    switch (action.action) {
      case "create-branch": {
        const problem = refNameProblem(action.name);
        const start = await this.resolve(action.start);
        p.refs.push({ name: branchRef(action.name), before: null, after: start });
        p.steps.push(`Creates the branch ${action.name} at ${start ? `${short(start)} ("${await this.subject(start)}")` : action.start}.`);
        stop(problem ? block("invalid-name", problem) : null);
        stop(!problem && (await this.branchExists(action.name)) ? block("exists", `A branch named ${action.name} exists already. Choose another name.`) : null);
        stop(start ? null : block("not-found", `There is no commit or branch "${action.start}".`));
        if (action.switch) {
          p.steps.push(`Switches to ${action.name}; new commits go there.`);
          p.refs.push({ name: "HEAD", before: head, after: start });
          if (start && start !== head) {
            p.backup = true;
            stop(this.unsaved());
            stop(await this.inProgress());
            stop(this.overwritten(await this.touched(head, start)));
            p.files = sorted(changedPaths(await flatTree(r, head), await flatTree(r, start))).map(slash);
          }
        }
        return p;
      }
      case "switch": {
        const t = action.target;
        const local = await this.branchExists(t);
        const remote = local ? null : await this.remoteOf(t);
        let target = await this.resolve(t);
        if (!target) return { ...p, blocked: block("not-found", `There is no branch, tag or commit "${t}".`) };
        if (local && t === current) return { ...p, undoable: false, blocked: block("nothing-to-do", `You are on ${t} already.`) };
        if (remote) {
          const name = t.slice(remote.length + 1);
          if (await this.branchExists(name)) {
            if (name === current) return { ...p, undoable: false, blocked: block("nothing-to-do", `You are on ${name} already.`) };
            p.steps.push(`Switches to your branch ${name} (it follows ${t}).`);
            target = (await refOid(r, branchRef(name)))!;
          } else {
            p.steps.push(`Creates the branch ${name} from ${t}, linked to it, and switches to it.`);
            p.refs.push({ name: branchRef(name), before: null, after: target });
          }
        } else if (local) {
          p.steps.push(`Switches to ${t}; your files show its state.`);
        } else {
          p.steps.push(`Shows the files of ${t} (${short(target)}). You will not be on a branch: create one there to keep new commits.`);
          p.warnings = [`${t} is not a branch.`];
        }
        p.refs.push({ name: "HEAD", before: head, after: target });
        p.files = sorted(changedPaths(await flatTree(r, head), await flatTree(r, target))).map(slash);
        p.backup = true;
        stop(this.unsaved());
        stop(await this.inProgress());
        stop(this.overwritten(await this.touched(head, target)));
        return p;
      }
      case "rename-branch": {
        const at = await refOid(r, branchRef(action.from));
        p.steps.push(`Renames ${action.from} to ${action.to}.`);
        p.refs.push({ name: branchRef(action.from), before: at, after: null }, { name: branchRef(action.to), before: null, after: at });
        if (await upstreamOf(r, action.from)) p.warnings = [`The server keeps ${action.from}; publish ${action.to} to put the new name there.`];
        p.backup = at !== null;
        const problem = refNameProblem(action.to);
        stop(at ? null : block("not-found", `There is no branch ${action.from}.`));
        stop(problem ? block("invalid-name", problem) : null);
        stop(!problem && (await this.branchExists(action.to)) ? block("exists", `A branch named ${action.to} exists already.`) : null);
        return p;
      }
      case "delete-branch": {
        const at = await refOid(r, branchRef(action.name));
        if (at === null) return { ...p, blocked: block("not-found", `There is no branch ${action.name}.`) };
        if (action.name === current) {
          return { ...p, blocked: block("current-branch", `You are on ${action.name}. Switch to another branch before you delete it.`) };
        }
        const only = await this.onlyOn(at, action.name);
        const tracked = (await upstreamOf(r, action.name)) !== null;
        p.steps.push(`Deletes the branch ${action.name} on this computer${tracked ? " (the server keeps its copy)" : ""}.`);
        p.refs.push({ name: branchRef(action.name), before: at, after: null });
        p.backup = true;
        if (only > 0) {
          p.destructive = true;
          p.warnings = [`${plural(only, "commit is", "commits are")} only on ${action.name}. The backup keeps ${only === 1 ? "it" : "them"} for Undo.`];
          if (!action.force) {
            p.blocked = block(
              "not-merged",
              `${action.name} has ${plural(only, "commit")} that ${only === 1 ? "is" : "are"} on no other branch. Confirm to delete it anyway.`,
            );
          }
        }
        return p;
      }
      case "merge": {
        needBranch();
        const from = await this.resolve(action.from);
        if (!from) return { ...p, blocked: block("not-found", `There is no branch, tag or commit "${action.from}".`) };
        if (!head || !cur) return p;
        if (await isAncestor(r, from, head)) {
          return { ...p, undoable: false, blocked: block("nothing-to-do", `${cur} already contains everything from ${action.from}.`) };
        }
        const merge = await this.mergePreview(head, from);
        p.merge = merge;
        const incoming = merge.commits.length;
        const conflicts = merge.files.filter((f) => f.conflicts.length > 0).length;
        p.steps.push(
          merge.fast_forward
            ? `Moves ${cur} forward to ${action.from}: ${plural(incoming, "commit")} come in, no merge commit needed.`
            : `Brings ${plural(incoming, "commit")} from ${action.from} into ${cur} with a merge commit.`,
        );
        if (conflicts > 0) p.steps.push(`${plural(conflicts, "file needs", "files need")} your decision afterwards: the same fields were changed on both sides.`);
        p.refs.push({ name: branchRef(cur), before: head, after: merge.fast_forward ? from : "" });
        p.files = merge.files.map((f) => f.path);
        p.backup = true;
        stop(this.unsaved());
        stop(await this.inProgress());
        stop(await this.localChanges("merge"));
        return p;
      }
      case "pull": {
        needBranch();
        p.network = true;
        p.backup = true;
        const up = cur ? await upstreamOf(r, cur) : null;
        if (cur && !up) stop(block("no-upstream", `${cur} is not on the server yet. Publish it first.`));
        const upName = up ? `${up.remote}/${up.branch}` : null;
        p.steps.push(`Gets new commits from ${upName ?? "the server"} and merges them into ${cur ?? "your branch"}.`);
        if (cur && head) p.refs.push({ name: branchRef(cur), before: head, after: "" });
        if (up && head) {
          // What the last fetch brought; the server may have more by now.
          const theirs = await refOid(r, `refs/remotes/${up.remote}/${up.branch}`);
          if (theirs && !(await isAncestor(r, theirs, head))) {
            p.merge = await this.mergePreview(head, theirs);
            p.steps.push("What comes in is shown as of the last check with the server.");
          }
        }
        stop(this.unsaved());
        stop(await this.inProgress());
        return p;
      }
      case "push": {
        needBranch();
        p.network = true;
        p.undoable = false;
        const up = cur ? await upstreamOf(r, cur) : null;
        if (cur && !up) stop(block("no-upstream", `${cur} is not on the server yet. Publish it first.`));
        if (cur && up && head) {
          const name = `${up.remote}/${up.branch}`;
          const theirs = await refOid(r, `refs/remotes/${name}`);
          const ahead = (await commitsOnly(r, head, [theirs])).length;
          const behind = theirs ? (await commitsOnly(r, theirs, [head])).length : 0;
          p.steps.push(`Sends ${plural(ahead, "commit")} to ${name}. Sending cannot be undone from the app.`);
          p.refs.push({ name: `refs/remotes/${name}`, before: theirs, after: head });
          if (behind > 0 && ahead > 0) stop(block("diverged", `${name} has ${plural(behind, "commit")} you do not have. Bring both together first.`));
          else if (behind > 0) stop(block("rejected", `${name} has ${plural(behind, "new commit")}. Pull first, then push.`));
          else if (ahead === 0) stop(block("nothing-to-do", "Nothing to send: the server has all your commits."));
        }
        stop(await this.inProgress());
        return p;
      }
      case "publish": {
        p.network = true;
        p.undoable = false;
        const at = await refOid(r, branchRef(action.branch));
        const unborn = at === null && action.branch === current;
        stop(at === null && !unborn ? block("not-found", `There is no branch ${action.branch}.`) : null);
        stop(unborn ? block("nothing-to-do", "Make the first commit first.") : null);
        stop((await remoteUrl(r, action.remote)) ? null : block("not-found", `There is no remote ${action.remote}. Add it first.`));
        stop((await upstreamOf(r, action.branch)) ? block("nothing-to-do", `${action.branch} is on the server already.`) : null);
        p.steps.push(`Puts ${action.branch} on ${action.remote} and links the two, so Pull and Push work from now on.`);
        p.refs.push({ name: `refs/remotes/${action.remote}/${action.branch}`, before: null, after: at });
        return p;
      }
      case "fetch": {
        p.network = true;
        p.undoable = false;
        const name = action.remote ?? "origin";
        stop((await remoteUrl(r, name)) ? null : block("not-found", `There is no remote ${name}.`));
        p.steps.push(`Asks ${name} what is new. Your branches and files stay as they are.`);
        return p;
      }
      case "add-remote": {
        const problem = refNameProblem(action.name);
        stop(problem ? block("invalid-name", problem) : null);
        stop((await remoteNames(r)).includes(action.name) ? block("exists", `A remote named ${action.name} exists already.`) : null);
        stop(action.url.trim() ? null : block("invalid-name", "Enter the address of the repository on the server."));
        p.steps.push(`Links this repository to ${action.url.trim() || "the server"} as "${action.name}".`);
        return p;
      }
      case "remove-remote": {
        stop((await remoteNames(r)).includes(action.name) ? null : block("not-found", `There is no remote ${action.name}.`));
        p.steps.push(`Removes the link to ${action.name}. Nothing on the server changes.`);
        for (const ref of await remoteRefs(r, action.name)) p.refs.push({ name: ref, before: await refOid(r, ref), after: null });
        p.backup = p.refs.length > 0;
        return p;
      }
      case "rename-remote": {
        const problem = refNameProblem(action.to);
        const names = await remoteNames(r);
        stop(names.includes(action.from) ? null : block("not-found", `There is no remote ${action.from}.`));
        stop(problem ? block("invalid-name", problem) : null);
        stop(names.includes(action.to) ? block("exists", `A remote named ${action.to} exists already.`) : null);
        p.steps.push(`Renames the remote ${action.from} to ${action.to}.`);
        if (!p.blocked) {
          const prefix = `refs/remotes/${action.from}/`;
          for (const ref of await remoteRefs(r, action.from)) {
            const oid = await refOid(r, ref);
            p.refs.push({ name: ref, before: oid, after: null }, { name: `refs/remotes/${action.to}/${ref.slice(prefix.length)}`, before: null, after: oid });
          }
        }
        return p;
      }
      case "set-remote-url": {
        stop((await remoteNames(r)).includes(action.name) ? null : block("not-found", `There is no remote ${action.name}.`));
        stop(action.url.trim() ? null : block("invalid-name", "Enter the address of the repository on the server."));
        p.steps.push(`Changes the address of ${action.name} to ${action.url.trim()}.`);
        return p;
      }
      case "stash": {
        const files = await workFiles(r);
        stop(files.length === 0 ? block("nothing-to-do", "There are no changes to park.") : null);
        stop(head ? null : block("detached", "Make the first commit first."));
        stop(this.unsaved());
        stop(await this.inProgress());
        p.steps.push(`Parks ${plural(files.length, "change")} and makes your files match the last commit. Bring them back any time.`);
        p.refs.push({ name: STASH_REF, before: await refOid(r, STASH_REF), after: "" });
        p.files = files.map((f) => slash(f.path));
        return p;
      }
      case "unstash": {
        const list = await stashLines(r);
        const s = list[action.index];
        if (!s) return { ...p, blocked: block("not-found", "There are no such parked changes.") };
        const content = await stashContent(r, s.oid);
        p.steps.push(`Brings back "${parseStashMessage(s.message).message}" (${plural(content.files.size, "file")}).`);
        if (!action.keep) {
          p.steps.push("Removes it from the parked changes; a backup keeps it for Undo.");
          p.refs.push({ name: STASH_REF, before: list[0].oid, after: action.index === 0 ? (list[1]?.oid ?? null) : list[0].oid });
        }
        p.backup = true;
        p.files = sorted(content.files.keys()).map(slash);
        stop(this.unsaved());
        stop(await this.inProgress());
        const fit = await this.unstashWrites(content);
        stop(this.overwritten(fit.clash));
        stop(
          fit.unfit.length
            ? block(
                "unsupported",
                `${fit.unfit.join(", ")} changed since you parked the changes, and they do not fit together. Bring them back on the command line (git stash apply).`,
              )
            : null,
        );
        return p;
      }
      case "drop-stash": {
        const list = await stashLines(r);
        const s = list[action.index];
        if (!s) return { ...p, blocked: block("not-found", "There are no such parked changes.") };
        p.steps.push(`Deletes the parked changes "${parseStashMessage(s.message).message}".`);
        p.refs.push({ name: STASH_REF, before: list[0].oid, after: action.index === 0 ? (list[1]?.oid ?? null) : list[0].oid });
        p.backup = true;
        p.destructive = true;
        return p;
      }
      case "revert":
      case "cherry-pick": {
        needBranch();
        const oid = await this.resolve(action.commit);
        if (!oid) return { ...p, blocked: block("not-found", `There is no commit "${action.commit}".`) };
        if (!head || !cur) return p;
        const commit = await readCommit(r, oid);
        if (commit.parent.length > 1) {
          return { ...p, blocked: block("unsupported", "This is a merge commit. Undoing or copying merges works on the command line for now.") };
        }
        const pick = action.action === "cherry-pick";
        if (commit.parent.length === 0 && !pick) {
          return { ...p, blocked: block("unsupported", "This is the very first commit; undoing it would remove everything.") };
        }
        if (pick && (await isAncestor(r, oid, head))) return { ...p, undoable: false, blocked: block("nothing-to-do", `${cur} contains this commit already.`) };
        const parent = commit.parent[0] ?? null;
        const m = pick ? await computeMerge(r, parent, head, oid) : await computeMerge(r, oid, head, parent!);
        const subject = subjectOf(commit.message);
        p.steps.push(
          pick ? `Copies the changes of "${subject}" onto ${cur} as a new commit.` : `Makes a new commit on ${cur} that takes back "${subject}". History stays as it is.`,
        );
        const conflicts = m.files.filter((f) => f.conflicts.length > 0).length;
        if (conflicts > 0) {
          p.steps.push(`${plural(conflicts, "file needs", "files need")} your decision afterwards.`);
          p.warnings = [
            `The web app cannot finish a stopped ${pick ? "copy" : "undo"} yet: finish it on the command line (git ${pick ? "cherry-pick" : "revert"} --continue) or use Undo.`,
          ];
        }
        if (m.files.length === 0) stop(block("nothing-to-do", pick ? "These changes are on your branch already." : "Your branch no longer has these changes."));
        p.merge = { fast_forward: false, commits: [toCommit(oid, commit)], files: preview(m.files) };
        p.refs.push({ name: branchRef(cur), before: head, after: "" });
        p.files = m.files.map((f) => slash(f.path));
        p.backup = true;
        stop(this.unsaved());
        stop(await this.inProgress());
        stop(await this.localChanges(pick ? "copy a commit" : "undo a commit"));
        return p;
      }
      case "reset": {
        needBranch();
        const target = await this.resolve(action.target);
        if (!target) return { ...p, blocked: block("not-found", `There is no commit "${action.target}".`) };
        if (!head || !cur) return p;
        if (target === head && action.mode === "keep-changes") return { ...p, undoable: false, blocked: block("nothing-to-do", `${cur} is there already.`) };
        const leaving = await commitsOnly(r, head, [target]);
        p.steps.push(`Moves ${cur} to ${short(target)} ("${await this.subject(target)}").`);
        p.steps.push(
          action.mode === "keep-changes"
            ? "Your files stay as they are; what the removed commits did shows as changes."
            : "Your files match that commit; uncommitted changes are thrown away (the backup keeps them).",
        );
        const warnings: string[] = [];
        if (leaving.length > 0) {
          const n = leaving.length;
          warnings.push(`${plural(n, "commit leaves", "commits leave")} ${cur}. The backup keeps ${n === 1 ? "it" : "them"} for Undo.`);
          const up = await upstreamOf(r, cur);
          const theirs = up ? await refOid(r, `refs/remotes/${up.remote}/${up.branch}`) : null;
          if (theirs) {
            const published = await ancestors(r, [theirs]);
            if (leaving.some((c) => published.has(c.oid))) warnings.push("Some of these commits are on the server already. The next Pull brings them back.");
          }
        }
        if (warnings.length) p.warnings = warnings;
        p.refs.push({ name: branchRef(cur), before: head, after: target });
        p.backup = true;
        if (action.mode === "discard") {
          p.destructive = true;
          p.files = (await this.resetPaths(head, target)).map(slash);
          stop(this.unsaved());
        }
        stop(await this.inProgress());
        return p;
      }
      case "discard": {
        const all = await workFiles(r);
        const wanted = new Set(action.paths.length ? action.paths.map((x) => repoPath(r, x)) : all.map((f) => f.path));
        const hit = all.filter((f) => wanted.has(f.path));
        if (hit.length === 0) return { ...p, undoable: false, blocked: block("nothing-to-do", "There are no changes to discard.") };
        p.steps.push(`Throws away the changes to ${plural(hit.length, "file")}; they go back to the last commit.`);
        p.steps.push("A backup keeps them, so Undo can bring them back.");
        p.files = hit.map((f) => slash(f.path));
        p.backup = true;
        p.destructive = true;
        stop(this.unsaved());
        stop(hit.some((f) => f.state === "conflicted") ? block("in-progress", "Resolve the conflicts first.") : null);
        stop(head ? null : block("unsupported", "Make the first commit first: there is no version to go back to."));
        return p;
      }
      case "create-tag": {
        const problem = refNameProblem(action.name);
        const target = await this.resolve(action.target);
        stop(problem ? block("invalid-name", problem) : null);
        stop(!problem && (await refOid(r, `refs/tags/${action.name}`)) ? block("exists", `A tag named ${action.name} exists already.`) : null);
        stop(target ? null : block("not-found", `There is no commit "${action.target}".`));
        p.steps.push(`Names ${target ? short(target) : action.target} "${action.name}"${action.message?.trim() ? " with a message" : ""}. A tag never moves.`);
        p.refs.push({ name: `refs/tags/${action.name}`, before: null, after: target });
        return p;
      }
      case "delete-tag": {
        const t = await refOid(r, `refs/tags/${action.name}`);
        if (!t) return { ...p, blocked: block("not-found", `There is no tag ${action.name}.`) };
        p.steps.push(`Deletes the tag ${action.name} on this computer.`);
        if ((await remoteNames(r)).length > 0) p.warnings = ["If the server has the tag, it keeps its copy."];
        p.refs.push({ name: `refs/tags/${action.name}`, before: t, after: null });
        p.backup = true;
        return p;
      }
      case "push-tag": {
        p.network = true;
        p.undoable = false;
        stop((await refOid(r, `refs/tags/${action.name}`)) ? null : block("not-found", `There is no tag ${action.name}.`));
        stop((await remoteUrl(r, action.remote)) ? null : block("not-found", `There is no remote ${action.remote}.`));
        p.steps.push(`Sends the tag ${action.name} to ${action.remote}.`);
        return p;
      }
      default:
        return { ...p, undoable: false, blocked: block("unsupported", "This Git action is not supported here.") };
    }
  }

  /** `plan.merge` for bringing `from` into `head`. */
  private async mergePreview(head: string, from: string): Promise<NonNullable<GitPlan["merge"]>> {
    const r = this.r;
    const commits = await commitsOnly(r, from, [head], 50);
    if (await isAncestor(r, head, from)) {
      const [hf, ff] = [await flatTree(r, head), await flatTree(r, from)];
      const roots = collectionMerger() ? collectionRoots([hf, ff]) : [];
      const files = sorted(changedPaths(hf, ff)).map((path) => ({ path: slash(path), semantic: collectionOf(path, roots) !== null, clean: 1, conflicts: [] }));
      return { fast_forward: true, commits, files };
    }
    const [base] = (await git.findMergeBase({ ...b(r), oids: [head, from] })) as string[];
    const m = await computeMerge(r, base ?? null, head, from);
    return { fast_forward: false, commits, files: preview(m.files) };
  }

  /** Files a hard reset to `target` writes: what differs between the commits and every
   *  uncommitted change to a tracked file. */
  private async resetPaths(head: string, target: string): Promise<string[]> {
    const paths = changedPaths(await flatTree(this.r, head), await flatTree(this.r, target));
    for (const f of await workFiles(this.r)) if (f.state !== "untracked") paths.add(f.path);
    return sorted(paths);
  }

  /** What bringing back a stash writes, and what is in the way: a file with uncommitted changes
   *  is a clash; a file a commit changed since is merged line by line if it can be. */
  async unstashWrites(content: { base: Flat; files: Map<string, Entry | null> }): Promise<{ writes: Map<string, Content | null>; clash: string[]; unfit: string[] }> {
    const r = this.r;
    const headFlat = await flatTree(r, await headOid(r));
    const changed = new Set((await workFiles(r)).map((f) => f.path));
    const writes = new Map<string, Content | null>();
    const clash: string[] = [];
    const unfit: string[] = [];
    const merger = collectionMerger();
    for (const [p, want] of content.files) {
      const bytes = await readOptional(r, p);
      const cur = bytes ? (await git.hashBlob({ object: bytes })).oid : null;
      if (cur === (want?.oid ?? null)) continue;
      if (cur === (content.base.get(p)?.oid ?? null)) {
        writes.set(p, entryContent(want));
        continue;
      }
      if (changed.has(p) && cur !== (headFlat.get(p)?.oid ?? null)) {
        clash.push(p);
        continue;
      }
      const [bt, ot, tt] = [await blobText(r, content.base.get(p)), bytes ? utf8(bytes) : null, await blobText(r, want)];
      if (merger && ot !== null && tt !== null) {
        const line = await merger.text(bt, ot, tt);
        if (line.clean) {
          writes.set(p, { text: line.text });
          continue;
        }
      }
      unfit.push(p);
    }
    return { writes, clash: sorted(clash), unfit: sorted(unfit) };
  }

  // ------------------------------------------------------------ apply

  private async backup(t: number, name: string, oid: string | null, out: string[]): Promise<void> {
    if (!oid) return;
    const ref = backupRef(t, name);
    await git.writeRef({ fs: this.r.fs, dir: this.r.dir, ref, value: oid, force: true });
    out.push(ref);
  }

  /** Switches `HEAD` (to a branch ref, or detached for `null`) and the files to `to`. */
  private async checkoutTo(from: string | null, to: string, ref: string | null): Promise<void> {
    const r = this.r;
    await moveWork(r, await flatTree(r, from), await flatTree(r, to));
    if (ref) await git.writeRef({ fs: r.fs, dir: r.dir, ref: "HEAD", value: ref, symbolic: true, force: true });
    else await git.writeRef({ fs: r.fs, dir: r.dir, ref: "HEAD", value: to, force: true });
  }

  /** A snapshot of the working files, backed up as `worktree`; recorded in `x`. */
  private async snapshot(t: number, head: string | null, backups: string[], x: Extra): Promise<string | null> {
    if (!head) return null;
    const who = this.ctx.identity() ?? FALLBACK_WHO;
    const snap = await snapshotCommit(this.r, head, await currentBranch(this.r), who, t, "Backup of the working files");
    if (!snap) return null;
    await this.backup(t, "worktree", snap.oid, backups);
    x.work = snap.oid;
    return snap.oid;
  }

  private commitTree(
    tree: string,
    parents: string[],
    message: string,
    author: ReturnType<typeof signature>,
    committer: ReturnType<typeof signature> = author,
  ): Promise<string> {
    return git.writeCommit({
      fs: this.r.fs,
      dir: this.r.dir,
      commit: { tree, parent: parents, message: message.endsWith("\n") ? message : `${message}\n`, author, committer },
    });
  }

  private async push(net: Network, remote: string, ref: string, remoteRef: string): Promise<void> {
    const url = await remoteUrl(this.r, remote);
    if (!url) throw gitErr(`There is no remote ${remote}.`);
    const a = auth(net, url);
    try {
      const result = await git.push({
        ...b(this.r),
        http: net.http as never,
        remote,
        ref,
        remoteRef,
        onAuth: a.onAuth,
        onAuthFailure: a.onAuthFailure,
        onProgress: onProgress(net),
      });
      const refError = Object.values(result.refs ?? {}).find((x) => !x.ok)?.error;
      if (!result.ok || refError) {
        const why = refError ?? result.error ?? "rejected";
        if (/non-fast-forward|fetch first|rejected|already exists/i.test(why)) {
          throw gitErr(`${remote} has a different version already. Pull first, then send again.`);
        }
        throw gitErr(`The remote refused the push: ${why}`);
      }
    } catch (e) {
      throw ops.gitError(e, a.host);
    }
  }

  private async run(action: GitAction, p: GitPlan, t: number, backups: string[], x: Extra): Promise<Done> {
    const r = this.r;
    const head = await headOid(r);
    const cur = await currentBranch(r);
    const fsDir = { fs: r.fs, dir: r.dir };
    switch (action.action) {
      case "create-branch": {
        const start = (await this.resolve(action.start))!;
        await git.writeRef({ ...fsDir, ref: branchRef(action.name), value: start });
        if (action.switch) {
          if (p.backup) await this.backup(t, "HEAD", head, backups);
          await this.checkoutTo(head, start, branchRef(action.name));
          return { message: `Created ${action.name} and switched to it.`, reload: start !== head };
        }
        return { message: `Created ${action.name}.`, reload: false };
      }
      case "switch": {
        const name0 = action.target;
        await this.backup(t, "HEAD", head, backups);
        const target = (await this.resolve(name0))!;
        if (await this.branchExists(name0)) {
          await this.checkoutTo(head, target, branchRef(name0));
          return { message: `Switched to ${name0}.`, reload: true };
        }
        const remote = await this.remoteOf(name0);
        if (remote) {
          const name = name0.slice(remote.length + 1);
          let tip = await refOid(r, branchRef(name));
          if (!tip) {
            await git.writeRef({ ...fsDir, ref: branchRef(name), value: target });
            tip = target;
          }
          if (!(await upstreamOf(r, name))) {
            x.config = [await readSection(r, "branch", name)];
            await git.setConfig({ ...fsDir, path: `branch.${name}.remote`, value: remote });
            await git.setConfig({ ...fsDir, path: `branch.${name}.merge`, value: branchRef(name) });
          }
          await this.checkoutTo(head, tip, branchRef(name));
          return { message: `Switched to ${name}.`, reload: true };
        }
        await this.checkoutTo(head, target, null);
        return { message: `Showing ${name0}. You are not on a branch.`, reload: true };
      }
      case "rename-branch": {
        const at = (await refOid(r, branchRef(action.from)))!;
        await this.backup(t, action.from, at, backups);
        const before = await readSection(r, "branch", action.from);
        x.config = [before, await readSection(r, "branch", action.to)];
        await git.writeRef({ ...fsDir, ref: branchRef(action.to), value: at });
        if (before.values) await writeSection(r, { ...before, name: action.to });
        await clearSection(r, "branch", action.from);
        if (cur === action.from) await git.writeRef({ ...fsDir, ref: "HEAD", value: branchRef(action.to), symbolic: true, force: true });
        await git.deleteRef({ ...fsDir, ref: branchRef(action.from) });
        return { message: `Renamed ${action.from} to ${action.to}.`, reload: false };
      }
      case "delete-branch": {
        await this.backup(t, action.name, await refOid(r, branchRef(action.name)), backups);
        x.config = [await readSection(r, "branch", action.name)];
        await git.deleteRef({ ...fsDir, ref: branchRef(action.name) });
        await clearSection(r, "branch", action.name);
        return { message: `Deleted ${action.name}.`, reload: false };
      }
      case "merge": {
        const from = (await this.resolve(action.from))!;
        await this.backup(t, cur!, head, backups);
        if (p.merge?.fast_forward) {
          await moveWork(r, await flatTree(r, head), await flatTree(r, from));
          await git.writeRef({ ...fsDir, ref: branchRef(cur!), value: from, force: true });
          return { message: `Moved ${cur} forward to ${action.from}.`, outcome: { outcome: "fast-forward", commits: p.merge.commits.length }, reload: true };
        }
        const who = signature(this.identity(), t);
        const message = `Merge ${action.from} into ${cur}`;
        const [base] = (await git.findMergeBase({ ...b(r), oids: [head!, from] })) as string[];
        const tree = await writeMerge(r, await computeMerge(r, base ?? null, head!, from));
        if (tree === null) {
          await writeWorkFile(r, ".git/MERGE_HEAD", `${from}\n`);
          await writeWorkFile(r, ".git/MERGE_MSG", `${message}\n`);
          const paths = (await conflictedPaths(r)).map((x) => collectionPath(r, x));
          return { message: `Merging ${action.from} needs your decision in ${plural(paths.length, "file")}.`, outcome: { outcome: "conflicts", paths }, reload: true };
        }
        const oid = await this.commitTree(tree, [head!, from], message, who);
        await git.writeRef({ ...fsDir, ref: branchRef(cur!), value: oid, force: true });
        return { message: `Merged ${action.from} into ${cur}.`, outcome: { outcome: "merged", oid }, reload: true };
      }
      case "pull": {
        await this.backup(t, cur!, head, backups);
        const up = await upstreamOf(r, cur!);
        const upstream = up ? `${up.remote}/${up.branch}` : `origin/${cur}`;
        const res = await ops.pull(r, await this.ctx.network(), this.ctx.identity());
        const message =
          res.outcome === "up-to-date"
            ? "Already up to date."
            : res.outcome === "fast-forward"
              ? `Got ${plural(res.commits, "commit")} from ${upstream}.`
              : res.outcome === "merged"
                ? `Merged ${upstream}.`
                : `Conflicts in ${plural(res.paths.length, "file")}: choose a side for each, then commit.`;
        return { message, outcome: res, reload: res.outcome !== "up-to-date" };
      }
      case "push": {
        const up = await upstreamOf(r, cur!);
        await ops.push(r, await this.ctx.network());
        return { message: `Sent ${cur} to ${up ? `${up.remote}/${up.branch}` : `origin/${cur}`}.`, reload: false };
      }
      case "publish": {
        const at = (await refOid(r, branchRef(action.branch)))!;
        await this.push(await this.ctx.network(), action.remote, branchRef(action.branch), branchRef(action.branch));
        await git.writeRef({ ...fsDir, ref: `refs/remotes/${action.remote}/${action.branch}`, value: at, force: true });
        await git.setConfig({ ...fsDir, path: `branch.${action.branch}.remote`, value: action.remote });
        await git.setConfig({ ...fsDir, path: `branch.${action.branch}.merge`, value: branchRef(action.branch) });
        return { message: `Published ${action.branch} to ${action.remote}.`, reload: false };
      }
      case "fetch": {
        const name = action.remote ?? "origin";
        const url = (await remoteUrl(r, name))!;
        const net = await this.ctx.network();
        const a = auth(net, url);
        try {
          await git.fetch({ ...b(r), http: net.http as never, remote: name, singleBranch: false, tags: true, onAuth: a.onAuth, onAuthFailure: a.onAuthFailure, onProgress: onProgress(net) });
        } catch (e) {
          throw ops.gitError(e, a.host);
        }
        return { message: "Checked the server for changes.", reload: false };
      }
      case "add-remote": {
        x.config = [await readSection(r, "remote", action.name)];
        await git.addRemote({ ...fsDir, remote: action.name, url: action.url.trim() });
        return { message: `Linked to ${action.url.trim()} as ${action.name}.`, reload: false };
      }
      case "remove-remote": {
        const tracking = await trackingBranches(r, action.name);
        x.config = [await readSection(r, "remote", action.name)];
        for (const n of tracking) x.config.push(await readSection(r, "branch", n));
        for (const m of p.refs) {
          await this.backup(t, m.name.slice("refs/".length), m.before, backups);
          await deleteRefIfAny(r, m.name);
        }
        await deleteRefIfAny(r, `refs/remotes/${action.name}/HEAD`);
        await clearSection(r, "remote", action.name);
        for (const n of tracking) {
          await git.setConfig({ ...fsDir, path: `branch.${n}.remote`, value: undefined });
          await git.setConfig({ ...fsDir, path: `branch.${n}.merge`, value: undefined });
        }
        return { message: `Removed the remote ${action.name}.`, reload: false };
      }
      case "rename-remote": {
        const from = await readSection(r, "remote", action.from);
        const tracking = await trackingBranches(r, action.from);
        x.config = [from, await readSection(r, "remote", action.to)];
        for (const n of tracking) x.config.push(await readSection(r, "branch", n));
        const values: Record<string, string[]> = {};
        for (const [key, list] of Object.entries(from.values ?? {})) {
          values[key] = key === "fetch" ? list.map((v) => v.split(`refs/remotes/${action.from}/`).join(`refs/remotes/${action.to}/`)) : list;
        }
        await writeSection(r, { section: "remote", name: action.to, values });
        await clearSection(r, "remote", action.from);
        for (const n of tracking) await git.setConfig({ ...fsDir, path: `branch.${n}.remote`, value: action.to });
        for (const m of p.refs) await setRef(r, m.name, m.after);
        await deleteRefIfAny(r, `refs/remotes/${action.from}/HEAD`);
        return { message: `Renamed the remote ${action.from} to ${action.to}.`, reload: false };
      }
      case "set-remote-url": {
        x.config = [await readSection(r, "remote", action.name)];
        await git.setConfig({ ...fsDir, path: `remote.${action.name}.url`, value: action.url.trim() });
        return { message: `${action.name} now points to ${action.url.trim()}.`, reload: false };
      }
      case "stash": {
        const text = action.message?.trim() || `Parked on ${cur ?? "HEAD"}`;
        const who = this.ctx.identity() ?? FALLBACK_WHO;
        x.stashLog = await readText(r, STASH_LOG);
        const snap = (await snapshotCommit(r, head!, cur, who, t, text))!;
        const old = await refOid(r, STASH_REF);
        await git.writeRef({ ...fsDir, ref: STASH_REF, value: snap.oid, force: true });
        await writeWorkFile(r, STASH_LOG, `${x.stashLog ?? ""}${old ?? ZERO} ${snap.oid} ${reflogRest(who, t, snap.message)}\n`);
        const headFlat = await flatTree(r, head);
        for (const f of snap.files) await writeContent(r, f, entryContent(headFlat.get(f)));
        await indexToHead(r, snap.files, head);
        x.work = snap.oid;
        x.paths = snap.files;
        return { message: `Parked ${plural(snap.files.length, "change")}.`, reload: true };
      }
      case "unstash": {
        const list = await stashLines(r);
        const s = list[action.index];
        const fit = await this.unstashWrites(await stashContent(r, s.oid));
        await this.snapshot(t, head, backups, x);
        if (!action.keep) await this.backup(t, "stash", s.oid, backups);
        for (const [path, content] of fit.writes) await writeContent(r, path, content);
        x.paths = sorted(fit.writes.keys());
        if (!action.keep) {
          x.stashLog = await readText(r, STASH_LOG);
          list.splice(action.index, 1);
          await writeStashLines(r, list);
        }
        return { message: `Brought back "${parseStashMessage(s.message).message}".`, reload: true };
      }
      case "drop-stash": {
        const list = await stashLines(r);
        const [s] = list.splice(action.index, 1);
        await this.backup(t, "stash", s.oid, backups);
        x.stashLog = await readText(r, STASH_LOG);
        await writeStashLines(r, list);
        return { message: `Deleted the parked changes "${parseStashMessage(s.message).message}".`, reload: false };
      }
      case "revert":
      case "cherry-pick": {
        const oid = (await this.resolve(action.commit))!;
        const commit = await readCommit(r, oid);
        const pick = action.action === "cherry-pick";
        const subject = subjectOf(commit.message);
        const parent = commit.parent[0] ?? null;
        const message = pick ? commit.message : `Revert "${subject}"\n\nThis reverts commit ${oid}.\n`;
        const who = this.ctx.identity();
        const committer = who ? signature(who, t) : pick ? commit.committer : signature(this.identity(), t);
        const author = pick ? commit.author : committer;
        await this.backup(t, cur!, head, backups);
        const m = pick ? await computeMerge(r, parent, head!, oid) : await computeMerge(r, oid, head!, parent!);
        const tree = await writeMerge(r, m);
        if (tree === null) {
          await writeWorkFile(r, pick ? ".git/CHERRY_PICK_HEAD" : ".git/REVERT_HEAD", `${oid}\n`);
          await writeWorkFile(r, ".git/MERGE_MSG", message.endsWith("\n") ? message : `${message}\n`);
          const paths = (await conflictedPaths(r)).map((x) => collectionPath(r, x));
          return {
            message: `${pick ? "Copying" : "Undoing"} "${subject}" needs your decision in ${plural(paths.length, "file")}.`,
            outcome: { outcome: "conflicts", paths },
            reload: true,
          };
        }
        const created = await this.commitTree(tree, [head!], message, author, committer);
        await git.writeRef({ ...fsDir, ref: branchRef(cur!), value: created, force: true });
        return {
          message: pick ? `Copied "${subject}" onto ${cur}.` : `Undid "${subject}" with a new commit.`,
          outcome: { outcome: "merged", oid: created },
          reload: true,
        };
      }
      case "reset": {
        const target = (await this.resolve(action.target))!;
        await this.backup(t, cur!, head, backups);
        if (action.mode === "discard") {
          const paths = await this.resetPaths(head!, target);
          const changed = new Set((await workFiles(r)).map((f) => f.path));
          if (changed.size > 0) {
            await this.snapshot(t, head, backups, x);
            x.paths = paths.filter((f) => changed.has(f));
          }
          const to = await flatTree(r, target);
          for (const f of paths) await writeContent(r, f, entryContent(to.get(f)));
          await git.writeRef({ ...fsDir, ref: branchRef(cur!), value: target, force: true });
          await indexToHead(r, paths, target);
        } else {
          const paths = changedPaths(await flatTree(r, head), await flatTree(r, target));
          await git.writeRef({ ...fsDir, ref: branchRef(cur!), value: target, force: true });
          await indexToHead(r, paths, target);
          x.keepWork = true;
        }
        return { message: `Moved ${cur} to ${short(target)}.`, reload: action.mode === "discard" };
      }
      case "discard": {
        await this.snapshot(t, head, backups, x);
        const paths = (p.files ?? []).map(unslash);
        const headFlat = await flatTree(r, head);
        for (const f of paths) await writeContent(r, f, entryContent(headFlat.get(f)));
        await indexToHead(r, paths, head);
        x.paths = paths;
        return { message: `Discarded the changes to ${plural(paths.length, "file")}.`, reload: true };
      }
      case "create-tag": {
        const target = (await this.resolve(action.target))!;
        const message = action.message?.trim();
        if (message) await git.annotatedTag({ ...fsDir, ref: action.name, object: target, message, tagger: signature(this.identity(), t) });
        else await git.tag({ ...fsDir, ref: action.name, object: target });
        return { message: `Tagged ${short(target)} as ${action.name}.`, reload: false };
      }
      case "delete-tag": {
        await this.backup(t, `tags/${action.name}`, await refOid(r, `refs/tags/${action.name}`), backups);
        await git.deleteRef({ ...fsDir, ref: `refs/tags/${action.name}` });
        return { message: `Deleted the tag ${action.name}.`, reload: false };
      }
      case "push-tag": {
        const ref = `refs/tags/${action.name}`;
        await this.push(await this.ctx.network(), action.remote, ref, ref);
        return { message: `Sent the tag ${action.name} to ${action.remote}.`, reload: false };
      }
      default:
        throw gitErr("This Git action is not supported here.");
    }
  }

  /** Refs to record: what the plan names (remote-tracking refs only for the remote actions),
   *  `HEAD`, the current branch and the stash. */
  private refNames(action: GitAction, p: GitPlan, cur: string | null): string[] {
    const remotes = action.action === "remove-remote" || action.action === "rename-remote";
    const names = new Set(p.refs.map((m) => m.name).filter((n) => remotes || !n.startsWith("refs/remotes/")));
    names.add("HEAD");
    if (cur) names.add(branchRef(cur));
    names.add(STASH_REF);
    return [...names];
  }

  async apply(action: GitAction): Promise<GitActionResult> {
    const r = this.r;
    const p = await this.plan(action);
    if (p.blocked) throw gitErr(p.blocked.message);
    const t = Math.floor(Date.now() / 1000);
    const names = this.refNames(action, p, await currentBranch(r));
    const before = new Map<string, string | null>();
    for (const n of names) before.set(n, await refValue(r, n));
    const x: Extra = { head: await rawHead(r) };
    const changedBefore = p.undoable ? new Set((await workFiles(r)).map((f) => f.path)) : new Set<string>();
    const backups: string[] = [];
    const done = await this.run(action, p, t, backups, x);
    if (done.reload) await this.reopen();
    if (!p.undoable) return { message: done.message, outcome: done.outcome, reload: done.reload };

    const now = await currentBranch(r);
    const moves: GitRefMove[] = [];
    const guard: [string, string | null][] = [];
    for (const name of new Set([...names, ...(now ? [branchRef(now)] : [])])) {
      const after = await refValue(r, name);
      // A branch switched to that the plan does not name did not move.
      const was = before.has(name) ? (before.get(name) ?? null) : after;
      if (was !== after) moves.push({ name, before: was, after });
      else guard.push([name, after]);
    }
    if (done.outcome?.outcome === "conflicts") {
      x.clearMerge = true;
      const after = (await workFiles(r)).map((f) => f.path).filter((f) => !changedBefore.has(f));
      x.paths = sorted([...(x.paths ?? []), ...after]);
    }
    // Files changed: undo writes them back, so the refs they came from must stay put.
    if (done.reload || x.clearMerge) x.guard = guard.filter(([n]) => n === "HEAD" || n.startsWith("refs/heads/"));
    const entry: Stored = { id: `${t}-${Math.random().toString(36).slice(2, 8)}`, action: action.action, summary: done.message, time: t, refs: moves };
    if (backups.length) entry.backups = backups;
    if (x.work) entry.stash = x.work;
    entry.web = x;
    await writeJournal(r, [...(await readJournal(r)), entry]);
    return { message: done.message, undo: publicEntry(entry), outcome: done.outcome, reload: done.reload };
  }

  private async reopen(): Promise<void> {
    await this.ctx.host.openHandle(this.ctx.host.session().handle);
  }

  // ------------------------------------------------------------ undo

  async lastAction(): Promise<GitUndoEntry | null> {
    if (!(await ops.isRepo(this.r))) return null;
    const journal = await readJournal(this.r);
    const e = journal[journal.length - 1];
    if (!e) return null;
    const stale = await staleness(this.r, e);
    return stale ? { ...publicEntry(e), stale } : publicEntry(e);
  }

  async undo(id: string): Promise<GitActionResult> {
    const r = this.r;
    const journal = await readJournal(r);
    const at = journal.findIndex((e) => e.id === id);
    if (at < 0) throw gitErr("This action can no longer be undone.");
    const e = journal[at];
    const stale = await staleness(r, e);
    if (stale) throw gitErr(`Cannot undo: ${stale}`);
    const unsaved = this.unsaved();
    if (unsaved) throw gitErr(unsaved.message);
    const x = e.web ?? {};
    const headAfter = await headOid(r);
    const headMove = e.refs.find((m) => m.name === "HEAD");
    const headBefore = headMove ? headMove.before : headAfter;

    // Files undo writes back from the commits must not hold newer changes.
    const fromCommits =
      headAfter !== headBefore && !x.keepWork ? changedPaths(await flatTree(r, headAfter), await flatTree(r, headBefore)) : new Set<string>();
    const own = new Set(x.paths ?? []);
    const newer = (await workFiles(r)).map((f) => f.path).filter((f) => fromCommits.has(f) && !own.has(f));
    if (newer.length > 0) throw gitErr(`Cannot undo: your changes to ${newer.join(", ")} would be overwritten. Commit or park them first.`);

    const conflicted = x.clearMerge ? await conflictedPaths(r) : [];
    if (x.clearMerge) for (const f of [...IN_PROGRESS_FILES, ".git/MERGE_MSG"]) await removeWorkFile(r, f);
    for (const s of x.config ?? []) await writeSection(r, s);
    for (const m of e.refs) if (m.name !== "HEAD") await setRef(r, m.name, m.before);
    if (x.stashLog !== undefined) {
      if (x.stashLog === null) await removeWorkFile(r, STASH_LOG);
      else await writeWorkFile(r, STASH_LOG, x.stashLog);
    }
    if (x.head && x.head !== (await rawHead(r))) await writeHead(r, x.head);
    else if (!x.head && headMove?.before && (await currentBranch(r)) === null) await writeHead(r, headMove.before);
    const head = await headOid(r);

    const touched = new Set<string>();
    if (headAfter !== head) {
      const [af, bf] = [await flatTree(r, headAfter), await flatTree(r, head)];
      for (const p of changedPaths(af, bf)) {
        if (!x.keepWork) await writeContent(r, p, entryContent(bf.get(p)));
        touched.add(p);
      }
    }
    if (x.paths?.length) {
      const snap = x.work ? await stashContent(r, x.work) : null;
      const headFlat = await flatTree(r, head);
      for (const p of x.paths) {
        const entry = snap ? (snap.files.has(p) ? snap.files.get(p) : snap.base.get(p)) : headFlat.get(p);
        await writeContent(r, p, entryContent(entry));
        touched.add(p);
      }
    }
    await indexToHead(r, [...touched, ...conflicted], head);
    journal.splice(at, 1);
    await writeJournal(r, journal);
    const reload = (touched.size > 0 && !x.keepWork) || (x.paths?.length ?? 0) > 0;
    if (reload) await this.reopen();
    return { message: `Undone: ${e.summary}`, reload };
  }
}

// ---------------------------------------------------------------- lists

async function listRemotes(r: Repo): Promise<GitRemote[]> {
  if (!(await ops.isRepo(r))) return [];
  const out: GitRemote[] = [];
  for (const { remote, url } of await git.listRemotes({ fs: r.fs, dir: r.dir })) {
    const push = await getConfig(r, `remote.${remote}.pushurl`);
    out.push({ name: remote, url: withoutCredentials(url ?? ""), ...(push ? { push_url: withoutCredentials(push) } : {}) });
  }
  return out;
}

async function listStashes(r: Repo): Promise<GitStash[]> {
  if (!(await ops.isRepo(r))) return [];
  const out: GitStash[] = [];
  for (const [index, s] of (await stashLines(r)).entries()) {
    const { branch, message } = parseStashMessage(s.message);
    const files = sorted((await stashContent(r, s.oid)).files.keys()).map(slash);
    out.push({ index, oid: s.oid, message, time: s.time, ...(branch ? { branch } : {}), files });
  }
  return out;
}

// ---------------------------------------------------------------- GitApi

export function webGitActions(ctx: WebGitActionContext): GitActionMethods {
  const session = async () => {
    await ready();
    return new Session(ctx.repo(), ctx);
  };
  return {
    remotes: () =>
      ctx.run(async () => {
        await ready();
        return listRemotes(ctx.repo());
      }),
    stashes: () =>
      ctx.run(async () => {
        await ready();
        return listStashes(ctx.repo());
      }),
    plan: (action) => ctx.run(async () => (await session()).plan(action)),
    apply: (action) => ctx.run(async () => (await session()).apply(action)),
    lastAction: () => ctx.run(async () => (await session()).lastAction()),
    undo: (id) => ctx.run(async () => (await session()).undo(id)),
  };
}
