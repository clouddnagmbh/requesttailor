// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B: the working copy, the file tree and file contents of the web app
// (`GitApi.workingCopy`, `fileTree`, `blob`) on isomorphic-git, with the answers of the desktop
// (`rt-git/src/{working_copy,file_tree,blob}.rs`): the whole work tree against `HEAD` and the
// index, paths repository-relative with a leading `/`, ignored files never listed.
//
// Differences from the desktop, from what isomorphic-git can see: no submodules (a submodule
// folder is a folder), and a file is "modified" when its content differs from `HEAD` (the
// executable bit is not compared). isomorphic-git loads with the first call, like ./graph.ts.

import {
  BackendError,
  type GitBlob,
  type GitFileState,
  type GitOperation,
  type GitTreeEntry,
  type GitWorkingCopy,
  type GitWorkingFile,
  type GitWorkingMethods,
} from "@requesttailor/ui";

import { BLOB_IMAGE_LIMIT, BLOB_TEXT_LIMIT, describeBlob } from "./blob-info";
import type { WebGitContext } from "./graph";
import type { Repo } from "./repo";
import { scopedPath } from "./scope";

type Git = typeof import("isomorphic-git").default;
type Modules = { git: Git; repo: typeof import("./repo"); rev: typeof import("./revisions") };

let loading: Promise<Modules> | null = null;
function modules(): Promise<Modules> {
  loading ??= (async (): Promise<Modules> => {
    await import("./buffer-global");
    return { git: (await import("isomorphic-git")).default, repo: await import("./repo"), rev: await import("./revisions") };
  })().catch((e: unknown) => {
    loading = null;
    throw e;
  });
  return loading;
}

const gitErr = (message: string) => new BackendError("git", message);
const COLLECTION_FILE = "opencollection.yml";
/** At most this many files in the list (`WORKING_COPY_LIMIT`). */
export const WORKING_COPY_LIMIT = 10_000;
/** More new files than this are not hashed to look for renames. */
const RENAME_CANDIDATES = 2_000;
/** A path that exists in no repository: asks `repo.status` for the branch facts only. */
const NO_FILES = "\u0000-no-files";

function base(repo: Repo) {
  return { fs: repo.fs, dir: repo.dir, cache: repo.cache };
}

const abs = (repo: Repo, rel: string) => `${repo.dir.replace(/\/+$/, "")}/${rel}`;

function notFound(e: unknown): boolean {
  const code = (e as { code?: string }).code;
  return code === "ENOENT" || code === "ENOTDIR" || code === "NotFoundError";
}

async function readFile(repo: Repo, rel: string): Promise<Uint8Array | null> {
  try {
    return (await repo.fs.promises.readFile(abs(repo, rel))) as Uint8Array;
  } catch (e) {
    if (notFound(e) || (e as { code?: string }).code === "EISDIR") return null;
    throw e;
  }
}

async function exists(repo: Repo, rel: string): Promise<boolean> {
  try {
    await repo.fs.promises.stat(abs(repo, rel));
    return true;
  } catch (e) {
    if (notFound(e)) return false;
    throw e;
  }
}

/** Repository path of an API path (`/a/b`), checked. */
function repoPathOf(path: string): string {
  const p = path.replace(/^\/+/, "");
  if (p.includes("\\") || p.split("/").some((s) => s === "." || s === ".." || s.toLowerCase() === ".git" || (s === "" && p !== ""))) {
    throw gitErr(`Invalid path "${path}".`);
  }
  return p;
}

async function headOid(g: Modules, repo: Repo): Promise<string | null> {
  try {
    return await g.git.resolveRef({ fs: repo.fs, dir: repo.dir, ref: "HEAD" });
  } catch {
    return null;
  }
}

async function operationOf(repo: Repo, merging: boolean): Promise<GitOperation | undefined> {
  if (merging) return "merge";
  if (await exists(repo, ".git/CHERRY_PICK_HEAD")) return "cherry-pick";
  if (await exists(repo, ".git/REVERT_HEAD")) return "revert";
  if ((await exists(repo, ".git/rebase-merge")) || (await exists(repo, ".git/rebase-apply"))) return "rebase";
  return undefined;
}

/** The collection files of the active collection (outside nested collections) name it. */
function collectionNamer(repo: Repo, paths: string[], collection: string | null, treePath: (p: string) => boolean) {
  if (!collection) return () => undefined;
  const nested = paths
    .filter((p) => p.endsWith(`/${COLLECTION_FILE}`))
    .map((p) => p.slice(0, -COLLECTION_FILE.length))
    // Only folders below the collection's own folder are nested collections.
    .filter((dir) => {
      const rel = scopedPath(repo, dir);
      return rel !== null && rel !== "";
    });
  return (p: string): string | undefined => {
    const rel = scopedPath(repo, p);
    if (rel === null || !treePath(rel)) return undefined;
    return nested.some((dir) => p.startsWith(dir)) ? undefined : collection;
  };
}

/** The working copy of the whole work tree of `repo` (its `scope` is only the collection's
 *  place, used to name collection files). */
export async function workingCopy(repo: Repo, collection: string | null, repository: string): Promise<GitWorkingCopy> {
  const g = await modules();
  const whole: Repo = { ...repo, scope: undefined };
  // Branch, upstream, ahead/behind and a merge in progress: from `repo.status` on a path that
  // lists no files (the matrix below is the one that reads the tree).
  const info = await g.repo.status({ ...whole, scope: NO_FILES });
  if (!info.repo) throw gitErr("This folder is not a Git repository.");
  const head = await headOid(g, repo);
  const detached = head !== null && ((await g.git.currentBranch({ ...base(repo), fullname: false })) ?? null) === null;

  const unmerged = new Set<string>();
  const index = await readFile(repo, ".git/index");
  if (index) for (const p of g.repo.unmergedPaths(index)) unmerged.add(p);

  const matrix = await g.git.statusMatrix(base(repo));
  const nameOf = collectionNamer(
    { ...repo, scope: repo.scope ?? "" },
    matrix.map((r) => r[0]),
    collection,
    g.rev.treePath,
  );
  const rows: GitWorkingFile[] = [];
  const newRows: number[] = [];
  const deleted = new Map<string, number>();
  for (const [filepath, h, w, s] of matrix) {
    let state: GitFileState | null = null;
    if (unmerged.has(filepath)) state = "conflicted";
    else if (h === 0 && w !== 0) state = "untracked";
    else if (h === 1 && w === 0) state = "deleted";
    else if (h === 1 && w === 2) state = "modified";
    if (!state) continue;
    const staged: GitWorkingFile["staged"] =
      h === 0 ? (s !== 0 ? "added" : undefined) : s === 0 ? "deleted" : s === 1 ? undefined : "modified";
    const sameAsIndex = (w === 1 && s === 1) || (w === 2 && s === 2);
    const unstaged = state === "conflicted" ? true : s === 0 ? false : w === 0 ? true : !sameAsIndex;
    const path = `/${filepath}`;
    const col = nameOf(filepath);
    rows.push({ path, state, unstaged, ...(staged ? { staged } : {}), ...(col ? { collection: col } : {}) });
    if (state === "untracked") newRows.push(rows.length - 1);
    if (state === "deleted") deleted.set(filepath, rows.length - 1);
  }
  for (const p of unmerged) {
    if (!rows.some((r) => r.path === `/${p}`)) rows.push({ path: `/${p}`, state: "conflicted", unstaged: true });
  }

  // Renames: a deleted file whose content shows up under a new name; both rows stay.
  if (deleted.size > 0 && head && newRows.length <= RENAME_CANDIDATES) {
    const byId = new Map<string, number[]>();
    for (const [filepath, at] of deleted) {
      try {
        const { oid } = await g.git.readBlob({ ...base(repo), oid: head, filepath });
        byId.set(oid, [...(byId.get(oid) ?? []), at]);
      } catch {
        // Not a blob in HEAD (a submodule): no rename.
      }
    }
    const taken = new Set<number>();
    for (const at of newRows) {
      const bytes = await readFile(repo, rows[at].path.slice(1));
      if (!bytes || bytes.length > BLOB_TEXT_LIMIT) continue;
      const { oid } = await g.git.hashBlob({ object: bytes });
      const old = byId.get(oid)?.find((i) => !taken.has(i));
      if (old === undefined) continue;
      taken.add(old);
      rows[at].from = rows[old].path;
      if (rows[at].staged === "added" && rows[old].staged === "deleted") rows[at].staged = "renamed";
    }
  }

  rows.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  const operation = await operationOf(repo, info.merging);
  const truncated = rows.length > WORKING_COPY_LIMIT;
  return {
    repository,
    branch: info.branch,
    upstream: info.upstream,
    ahead: info.ahead,
    behind: info.behind,
    ...(head ? { head } : {}),
    detached,
    ...(operation ? { operation } : {}),
    files: truncated ? rows.slice(0, WORKING_COPY_LIMIT) : rows,
    truncated,
  };
}

/** The entries of the folder `dir` (`""` or `/`: the top) at `rev`, or in the working tree. */
export async function fileTree(repo: Repo, rev: string | null, dir: string): Promise<GitTreeEntry[]> {
  const g = await modules();
  const folder = repoPathOf(dir.replace(/\/+$/, ""));
  const join = (name: string) => `/${folder ? `${folder}/` : ""}${name}`;
  const order = (entries: GitTreeEntry[]) =>
    entries.sort((a, b) => {
      const d = (e: GitTreeEntry) => (e.kind === "dir" ? 0 : 1);
      return d(a) - d(b) || a.name.toLowerCase().localeCompare(b.name.toLowerCase()) || (a.name < b.name ? -1 : 1);
    });

  if (rev !== null) {
    const commit = await g.rev.revParse(repo, rev);
    let tree;
    try {
      tree = (await g.git.readTree({ ...base(repo), oid: commit, ...(folder ? { filepath: folder } : {}) })).tree;
    } catch (e) {
      if (notFound(e) || (e as { code?: string }).code === "ObjectTypeError") return [];
      throw e;
    }
    const out: GitTreeEntry[] = [];
    for (const e of tree) {
      const kind: GitTreeEntry["kind"] =
        e.type === "tree" ? "dir" : e.type === "commit" ? "submodule" : e.mode === "120000" ? "symlink" : "file";
      let size: number | undefined;
      if (kind === "file" && tree.length <= 500) size = (await g.git.readBlob({ ...base(repo), oid: e.oid })).blob.length;
      out.push({ name: e.path, path: join(e.path), kind, ...(size === undefined ? {} : { size }), ignored: false });
    }
    return order(out);
  }

  let names: string[];
  try {
    names = (await repo.fs.promises.readdir(abs(repo, folder).replace(/\/$/, ""))) as string[];
  } catch (e) {
    if (notFound(e)) throw gitErr(`The folder /${folder} does not exist.`);
    throw e;
  }
  const matrix = await g.git.statusMatrix({ ...base(repo), ...(folder ? { filepaths: [folder] } : {}) });
  const known = new Map<string, GitFileState | "clean">();
  const tracked = new Set<string>();
  for (const [filepath, h, w] of matrix) {
    const state: GitFileState | "clean" = h === 0 && w !== 0 ? "untracked" : h === 1 && w === 0 ? "deleted" : h === 1 && w === 2 ? "modified" : "clean";
    known.set(filepath, state);
    if (h === 1) tracked.add(filepath);
  }
  const below = (path: string) => {
    for (const p of known.keys()) if (p.startsWith(`${path}/`)) return true;
    return false;
  };
  const out: GitTreeEntry[] = [];
  const seen = new Set<string>();
  for (const name of names) {
    if (name === ".git") continue;
    seen.add(name);
    const rel = folder ? `${folder}/${name}` : name;
    const st = await repo.fs.promises.stat(abs(repo, rel));
    if (st.isDirectory()) {
      const ignored = !below(rel) && (await g.git.isIgnored({ ...base(repo), filepath: `${rel}/` }));
      out.push({ name, path: join(name), kind: "dir", ignored });
      continue;
    }
    const state = known.get(rel);
    const ignored = state === undefined;
    out.push({
      name,
      path: join(name),
      kind: "file",
      size: st.size,
      ...(state && state !== "clean" ? { state } : {}),
      ignored,
    });
  }
  for (const [filepath, state] of known) {
    const name = folder ? filepath.slice(folder.length + 1) : filepath;
    if (state === "deleted" && !name.includes("/") && !seen.has(name)) {
      out.push({ name, path: join(name), kind: "file", state, ignored: false });
    }
  }
  return order(out);
}

/** A file's content at `rev` (`null`: the working tree) for the viewers. */
export async function blob(repo: Repo, rev: string | null, path: string): Promise<GitBlob> {
  const g = await modules();
  const filepath = repoPathOf(path);
  if (filepath === "") throw gitErr("A path is needed.");
  const api = `/${filepath}`;
  if (rev !== null) {
    const commit = await g.rev.revParse(repo, rev);
    try {
      const { blob: bytes, oid } = await g.git.readBlob({ ...base(repo), oid: commit, filepath });
      return describeBlob(api, commit, bytes, oid);
    } catch (e) {
      if (notFound(e) || (e as { code?: string }).code === "ObjectTypeError") throw gitErr(`${api} does not exist at ${rev}.`);
      throw e;
    }
  }
  const bytes = await readFile(repo, filepath);
  if (!bytes) throw gitErr(`${api} does not exist in the working tree.`);
  const hashed = bytes.length <= Math.max(BLOB_TEXT_LIMIT, BLOB_IMAGE_LIMIT) ? (await g.git.hashBlob({ object: bytes })).oid : undefined;
  return describeBlob(api, undefined, bytes, hashed);
}

export function webGitFiles(ctx: WebGitContext): GitWorkingMethods {
  const collection = () => {
    try {
      return ctx.host.session().id;
    } catch {
      return null;
    }
  };
  // The id of the active repository (what `GitActionTarget.repository` names); the folder's
  // name only where the host does not know it.
  const repositoryName = () => ctx.host.activeRepository?.() ?? ctx.host.repository?.()?.handle.name ?? "";
  return {
    workingCopy: () => ctx.run(() => workingCopy(ctx.repo(), collection(), repositoryName())),
    fileTree: (rev, dir) => ctx.run(() => fileTree(ctx.repo(), rev, dir)),
    blob: (rev, path) => ctx.run(() => blob(ctx.repo(), rev, path)),
  };
}
