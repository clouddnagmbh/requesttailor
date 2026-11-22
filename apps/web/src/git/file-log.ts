// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GitApi.fileLog` of the web app, following renames and moves like `git log --follow`: the
// same rules as the desktop's `crates/rt-git/src/file_log.rs` (a path that exists in a commit
// but not in its first parent may have been renamed from the collection file the commit
// removed with the same content, else the most similar one with at least half of the larger
// file's lines in common; ties go to the smallest path).

import "./buffer-global";

import git from "isomorphic-git";

import type { GitCommit, GitFileChange, GitFileCommit } from "@requesttailor/ui";

import type { Repo } from "./repo";

/** Share of lines two versions need in common to count as the same file. */
export const SIMILARITY = 0.5;

function base(repo: Repo) {
  return { fs: repo.fs, dir: repo.dir, cache: repo.cache };
}

/** A collection file as the shells read them (`rt_app::git::tree_path`). */
function collectionFile(path: string): boolean {
  const parts = path.split("/");
  const file = parts.pop() ?? "";
  return file.endsWith(".yml") && parts.every((p) => p !== "" && !p.startsWith(".") && p !== "node_modules");
}

/** Lines in common over the lines of the larger file; `1` for equal files. */
export function similarity(a: string, b: string): number {
  const lines = (t: string) => {
    const all = t === "" ? [] : t.split(/\r?\n/);
    if (all.length > 0 && all[all.length - 1] === "") all.pop();
    return all;
  };
  const [la, lb] = [lines(a), lines(b)];
  const larger = Math.max(la.length, lb.length);
  if (larger === 0) return 1;
  const counts = new Map<string, number>();
  for (const l of la) counts.set(l, (counts.get(l) ?? 0) + 1);
  let common = 0;
  for (const l of lb) {
    const n = counts.get(l) ?? 0;
    if (n > 0) {
      counts.set(l, n - 1);
      common++;
    }
  }
  return common / larger;
}

function checkPath(path: string): string {
  const p = path.replace(/^\//, "");
  if (!p || p.includes("\\") || p.split("/").some((s) => s === "" || s === "." || s === ".." || s.toLowerCase() === ".git")) {
    throw new Error(`Invalid path "${path}".`);
  }
  return p;
}

async function blobOid(repo: Repo, commit: string, filepath: string): Promise<string | null> {
  try {
    return (await git.readBlob({ ...base(repo), oid: commit, filepath })).oid;
  } catch (e) {
    if ((e as { code?: string }).code === "NotFoundError") return null;
    throw e;
  }
}

/** Every regular file of a commit's tree (path → blob id), collection files only. */
async function collectionFiles(repo: Repo, commit: string): Promise<Map<string, string>> {
  const { commit: c } = await git.readCommit({ ...base(repo), oid: commit });
  const out = new Map<string, string>();
  const walk = async (tree: string, prefix: string) => {
    for (const e of (await git.readTree({ ...base(repo), oid: tree })).tree) {
      const p = prefix ? `${prefix}/${e.path}` : e.path;
      if (e.type === "tree") await walk(e.oid, p);
      else if (e.type === "blob" && (e.mode === "100644" || e.mode === "100755") && collectionFile(p)) out.set(p, e.oid);
    }
  };
  await walk(c.tree, "");
  return out;
}

async function text(repo: Repo, oid: string): Promise<string> {
  const { blob } = await git.readBlob({ ...base(repo), oid });
  return new TextDecoder("utf-8").decode(blob);
}

async function renamedFrom(repo: Repo, parent: string, commit: string, blob: string, path: string): Promise<string | null> {
  const [old, now] = [await collectionFiles(repo, parent), await collectionFiles(repo, commit)];
  const removed = [...old].filter(([p]) => !now.has(p) && p !== path).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  const same = removed.find(([, id]) => id === blob);
  if (same) return same[0];
  if (removed.length === 0) return null;
  const mine = await text(repo, blob);
  let best: [number, string] | null = null;
  for (const [p, id] of removed) {
    const s = similarity(mine, await text(repo, id));
    if (s >= SIMILARITY && (!best || s > best[0])) best = [s, p];
  }
  return best ? best[1] : null;
}

function toCommit(oid: string, c: { message: string; author: { name: string; timestamp: number } }): GitCommit {
  return { oid, summary: c.message.split("\n")[0], author: c.author.name, time: c.author.timestamp };
}

/** Commits of `HEAD`'s history in which `path` differs from the first parent, newest first,
 *  following renames (an entry's `path` is the file's path at that commit). */
export async function followFileLog(repo: Repo, path: string, limit: number): Promise<GitFileCommit[]> {
  const first = checkPath(path);
  let head: string;
  try {
    head = await git.resolveRef({ ...base(repo), ref: "HEAD" });
  } catch {
    return [];
  }
  const followed = new Map<string, string>([[head, first]]);
  const out: GitFileCommit[] = [];
  for (const entry of await git.log({ ...base(repo), ref: head })) {
    if (out.length >= limit) break;
    const herePath = followed.get(entry.oid) ?? first;
    const here = await blobOid(repo, entry.oid, herePath);
    const parents = entry.commit.parent;
    const parent = parents[0] ? await blobOid(repo, parents[0], herePath) : null;
    let parentPath = herePath;
    let change: GitFileChange | null = null;
    let from: string | undefined;
    if (!parent && here) {
      const old = parents[0] ? await renamedFrom(repo, parents[0], entry.oid, here, herePath) : null;
      if (old) {
        change = "renamed";
        from = old;
        parentPath = old;
      } else change = "added";
    } else if (parent && !here) change = "deleted";
    else if (parent && here && parent !== here) change = "modified";
    parents.forEach((p, i) => {
      if (!followed.has(p)) followed.set(p, i === 0 ? parentPath : herePath);
    });
    if (change) out.push({ commit: toCommit(entry.oid, entry.commit), path: herePath, change, ...(from ? { from } : {}) });
  }
  return out;
}
