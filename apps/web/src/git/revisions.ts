// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: reading any revision without a checkout, on isomorphic-git, with the
// same answers as the desktop (`rt-git/src/revisions.rs`, `file_log.rs`): `revParse`,
// `treeAt`, `show`, `fileLog`, `mergeBase`, `commits`. In the browser the repository root is
// the collection folder, so collection- and repository-relative paths are the same.
// `fileLog` follows renames (./file-log.ts).

import "./buffer-global";

import git from "isomorphic-git";

import { BackendError, type GitCommit, type GitFileCommit, type GitTree } from "@requesttailor/ui";

import { followFileLog } from "./file-log";
import type { Repo } from "./repo";

const gitErr = (message: string) => new BackendError("git", message);
const COLLECTION_FILE = "opencollection.yml";

function base(repo: Repo) {
  return { fs: repo.fs, dir: repo.dir, cache: repo.cache };
}

function toCommit(oid: string, c: { message: string; author: { name: string; timestamp: number } }): GitCommit {
  return { oid, summary: c.message.split("\n")[0], author: c.author.name, time: c.author.timestamp };
}

async function firstParent(repo: Repo, oid: string): Promise<string | null> {
  const { commit } = await git.readCommit({ ...base(repo), oid });
  return commit.parent[0] ?? null;
}

async function resolveName(repo: Repo, name: string): Promise<string | null> {
  try {
    const oid = await git.resolveRef({ ...base(repo), ref: name });
    // Annotated tags: peel to the commit.
    const { type, oid: peeled } = await git.readObject({ ...base(repo), oid });
    if (type === "tag") return (await git.readTag({ ...base(repo), oid: peeled })).tag.object;
    return oid;
  } catch {
    // Not a ref.
  }
  if (/^[0-9a-f]{4,40}$/.test(name)) {
    try {
      return await git.expandOid({ ...base(repo), oid: name });
    } catch {
      return null;
    }
  }
  return null;
}

/** The commit `rev` names (see `GitApi`): full or abbreviated id, branch, `origin/<b>`, ref or
 *  `HEAD`, each optionally followed by `~<n>` or `^`. */
export async function revParse(repo: Repo, rev: string): Promise<string> {
  const m = /^(.*?)((?:~\d*|\^)*)$/.exec(rev.trim());
  const name = m?.[1] ?? "";
  if (!name) throw gitErr("No revision given.");
  let oid = await resolveName(repo, name);
  if (!oid) throw gitErr(`Unknown revision "${rev}".`);
  try {
    const { type } = await git.readObject({ ...base(repo), oid, format: "parsed" });
    if (type !== "commit") throw gitErr(`"${rev}" does not name a commit.`);
  } catch (e) {
    if (e instanceof BackendError) throw e;
    throw gitErr(`Unknown revision "${rev}".`);
  }
  for (const step of (m?.[2] ?? "").match(/~\d*|\^/g) ?? []) {
    const n = step === "^" ? 1 : Number(step.slice(1) || "1");
    for (let i = 0; i < n; i++) {
      const parent: string | null = await firstParent(repo, oid);
      if (!parent) throw gitErr(`Unknown revision "${rev}".`);
      oid = parent;
    }
  }
  return oid;
}

/** Whether `GitTree` holds `path`: a `.yml` file outside hidden folders and `node_modules`
 *  (`rt_app::git::tree_path`). */
export function treePath(path: string): boolean {
  const parts = path.split("/");
  const file = parts.pop() ?? "";
  if (!file.endsWith(".yml")) return false;
  return parts.every((p) => p !== "" && !p.startsWith(".") && p !== "node_modules");
}

/** Drops nested collections (`rt_app::git::drop_nested`). */
export function dropNested(files: Record<string, string>): void {
  const nested = Object.keys(files)
    .filter((p) => p.endsWith(`/${COLLECTION_FILE}`))
    .map((p) => p.slice(0, -COLLECTION_FILE.length));
  if (nested.length === 0) return;
  for (const p of Object.keys(files)) if (nested.some((dir) => p.startsWith(dir))) delete files[p];
}

const decoder = new TextDecoder("utf-8", { fatal: true });

/** The collection's files at `rev`. */
export async function treeAt(repo: Repo, rev: string): Promise<GitTree> {
  const oid = await revParse(repo, rev);
  const { commit } = await git.readCommit({ ...base(repo), oid });
  const files: Record<string, string> = {};
  const walk = async (tree: string, prefix: string) => {
    for (const e of (await git.readTree({ ...base(repo), oid: tree })).tree) {
      const p = prefix ? `${prefix}/${e.path}` : e.path;
      if (e.type === "tree") {
        if (!e.path.startsWith(".") && e.path !== "node_modules") await walk(e.oid, p);
      } else if (e.type === "blob" && (e.mode === "100644" || e.mode === "100755") && treePath(p)) {
        const { blob } = await git.readBlob({ ...base(repo), oid: e.oid });
        try {
          files[p] = decoder.decode(blob);
        } catch {
          // Not UTF-8: not a collection file.
        }
      }
    }
  };
  await walk(commit.tree, "");
  dropNested(files);
  return { oid, files };
}

async function blobOf(repo: Repo, commit: string, filepath: string): Promise<{ oid: string; blob: Uint8Array } | null> {
  try {
    const r = await git.readBlob({ ...base(repo), oid: commit, filepath });
    return { oid: r.oid, blob: r.blob };
  } catch (e) {
    if ((e as { code?: string }).code === "NotFoundError") return null;
    throw e;
  }
}

function checkPath(path: string): string {
  const p = path.replace(/^\//, "");
  if (!p || p.includes("\\") || p.split("/").some((s) => s === "" || s === "." || s === ".." || s.toLowerCase() === ".git")) {
    throw gitErr(`Invalid path "${path}".`);
  }
  return p;
}

/** Text of `path` at `rev`; `null` if it does not exist there. */
export async function show(repo: Repo, rev: string, path: string): Promise<string | null> {
  const filepath = checkPath(path);
  const found = await blobOf(repo, await revParse(repo, rev), filepath);
  if (!found) return null;
  try {
    return decoder.decode(found.blob);
  } catch {
    throw gitErr(`${path} is not a text file.`);
  }
}

/** Commits of `HEAD`'s history (by commit time) in which `path` differs from the first
 *  parent, newest first, following renames (./file-log.ts). */
export function fileLog(repo: Repo, path: string, limit: number): Promise<GitFileCommit[]> {
  return followFileLog(repo, path, limit);
}

/** Best common ancestor of `a` and `b`; `null` if unrelated. */
export async function mergeBase(repo: Repo, a: string, b: string): Promise<string | null> {
  const [x, y] = [await revParse(repo, a), await revParse(repo, b)];
  if (x === y) return x;
  const bases = await git.findMergeBase({ ...base(repo), oids: [x, y] });
  return bases[0] ?? null;
}

/** Commits reachable from `to` along first parents but not from `from`, newest first. */
export async function commits(repo: Repo, from: string | null, to: string, limit: number): Promise<GitCommit[]> {
  const hidden = new Set<string>();
  if (from) {
    for (const e of await git.log({ ...base(repo), ref: await revParse(repo, from) })) hidden.add(e.oid);
  }
  const out: GitCommit[] = [];
  let oid: string | null = await revParse(repo, to);
  while (oid && !hidden.has(oid) && out.length < limit) {
    const { commit } = await git.readCommit({ ...base(repo), oid });
    out.push(toCommit(oid, commit));
    oid = commit.parent[0] ?? null;
  }
  return out;
}
