// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand C: blame, history search and reflog of the web app (`GitApi.blame`,
// `searchHistory`, `reflog`) on isomorphic-git, with the answers of the desktop
// (`crates/rt-git/src/{blame,history_search,reflog}.rs`).
//
// - Blame hands the lines of the file down the history, newest commit first; what a commit did
//   not change (by the core's `textDiff`, the diff the desktop uses too) goes on to the first
//   parent that has it, a file missing in a parent may have been renamed (`renamedFrom`, any
//   regular file).
// - The search walks the graph's order (./graph.ts); path and content look at what a commit
//   changed against its first parent, merges only match on message, author and time. Content
//   is `git log -S`-like: a line with the text was added or removed (lines that moved between
//   files do not count).
// - isomorphic-git writes no reflog, but repositories from elsewhere (a folder opened through
//   the File System Access API) have one: `.git/logs/<ref>` is read directly.
//
// Paths are repository-relative with a leading `/` (the browser's repository root is the
// repository's folder); without it they are relative to the collection.

import "./buffer-global";

import git from "isomorphic-git";

import {
  BackendError,
  type GitBlame,
  type GitBlameHunk,
  type GitCommit,
  type GitHistoryField,
  type GitHistoryHit,
  type GitHistoryMethods,
  type GitHistoryPage,
  type GitHistoryQuery,
  type GitReflogEntry,
} from "@requesttailor/ui";

import { renamedFrom } from "./file-log";
import { graph as graphPage } from "./graph";
import type { WebGitContext } from "./graph";
import type { Repo } from "./repo";
import { revParse } from "./revisions";
import { repoPath } from "./scope";

/** Commits a content search reads at most per call (`rt_app::git::history::HISTORY_SCAN_LIMIT`). */
export const HISTORY_SCAN_LIMIT = 20_000;
/** Paths named in a hit at most. */
const HIT_PATHS = 5;
/** Files of one commit whose content is looked at, and their size. */
const FILES_PER_COMMIT = 200;
const FILE_MAX_BYTES = 2 * 1024 * 1024;

const gitErr = (message: string) => new BackendError("git", message);

function base(repo: Repo) {
  return { fs: repo.fs, dir: repo.dir, cache: repo.cache };
}

const decoder = new TextDecoder("utf-8", { fatal: true });

/** Path inside the repository, checked like every path (`Scope::abs`). */
function historyPath(repo: Repo, path: string): string {
  const slash = path.startsWith("/");
  const p = path.replace(/^\/+/, "");
  if (!p || p.includes("\\") || p.includes("\0") || p.split("/").some((s) => s === "" || s === "." || s === ".." || s.toLowerCase() === ".git")) {
    throw gitErr(`Invalid path "${path}".`);
  }
  return slash ? p : repoPath(repo, p);
}

async function blobAt(repo: Repo, commit: string, filepath: string): Promise<{ oid: string; blob: Uint8Array } | null> {
  try {
    const { oid, blob } = await git.readBlob({ ...base(repo), oid: commit, filepath });
    return { oid, blob };
  } catch (e) {
    const code = (e as { code?: string }).code;
    if (code === "NotFoundError" || code === "ObjectTypeError") return null;
    throw e;
  }
}

function textOf(bytes: Uint8Array): string | null {
  if (bytes.includes(0)) return null;
  try {
    return decoder.decode(bytes);
  } catch {
    return null;
  }
}

/** Lines with their line breaks, as the line diff counts them. */
function lineCount(text: string): number {
  if (text === "") return 0;
  let n = 0;
  for (let i = text.indexOf("\n"); i >= 0; i = text.indexOf("\n", i + 1)) n++;
  return text.endsWith("\n") ? n : n + 1;
}

interface DiffShape {
  binary: boolean;
  hunks: { new_start: number; new_lines: number; lines: { kind: string }[] }[];
}

/** Where each line of the new text is in the old one (`-1` for a line the diff added). */
function unchanged(diff: DiffShape, newLines: number): Int32Array | null {
  if (diff.binary) return null;
  const map = new Int32Array(newLines).fill(-1);
  let o = 0;
  let n = 0;
  const same = (until: number) => {
    while (n < until) {
      if (n < newLines) map[n] = o;
      o++;
      n++;
    }
  };
  for (const h of diff.hunks) {
    // An empty side of a hunk starts at the line before it (unified-diff convention).
    same(h.new_lines === 0 ? h.new_start : h.new_start - 1);
    for (const line of h.lines) {
      if (line.kind === "context") {
        if (n < newLines) map[n] = o;
        o++;
        n++;
      } else if (line.kind === "added") n++;
      else o++;
    }
  }
  same(newLines);
  return map;
}

interface CommitMeta {
  oid: string;
  parents: string[];
  time: number;
  commit: GitCommit;
}

async function metaOf(repo: Repo, oid: string, cache: Map<string, CommitMeta>): Promise<CommitMeta> {
  const hit = cache.get(oid);
  if (hit) return hit;
  const { commit } = await git.readCommit({ ...base(repo), oid });
  const meta: CommitMeta = {
    oid,
    parents: commit.parent,
    time: commit.committer.timestamp,
    commit: { oid, summary: commit.message.split("\n")[0].trim(), author: commit.author.name, time: commit.author.timestamp },
  };
  cache.set(oid, meta);
  return meta;
}

/** A line of the final file that is not blamed yet: [its index, its index in the commit]. */
type Line = [number, number];
interface Work {
  oid: string;
  path: string;
  lines: Line[];
  time: number;
  seq: number;
}

async function blame(ctx: WebGitContext, rev: string, path: string): Promise<GitBlame> {
  const repo = ctx.repo();
  const commit = await revParse(repo, rev);
  const first = historyPath(repo, path);
  const here = await blobAt(repo, commit, first);
  if (!here) throw gitErr(`/${first} does not exist in ${rev}`);
  const out: GitBlame = { path: `/${first}`, rev: commit, lines: 0, hunks: [], binary: false };
  const text = textOf(here.blob);
  if (text === null) return { ...out, binary: true };
  const total = lineCount(text);
  out.lines = total;
  if (total === 0) return out;
  const core = await ctx.host.deps.loadCore();

  const metas = new Map<string, CommitMeta>();
  const blamed: ([string, string, number] | null)[] = new Array<[string, string, number] | null>(total).fill(null);
  const pending = new Map<string, Work>();
  let seq = 0;
  const push = (oid: string, p: string, time: number, lines: Line[]) => {
    const key = `${oid}\0${p}`;
    const existing = pending.get(key);
    if (existing) existing.lines.push(...lines);
    else pending.set(key, { oid, path: p, lines, time, seq: seq++ });
  };
  push(commit, first, (await metaOf(repo, commit, metas)).time, Array.from({ length: total }, (_, i): Line => [i, i]));

  while (pending.size > 0) {
    // The newest commit first; ties in the order they were queued.
    let item: Work | null = null;
    for (const w of pending.values()) if (!item || w.time > item.time || (w.time === item.time && w.seq < item.seq)) item = w;
    const work = item!;
    pending.delete(`${work.oid}\0${work.path}`);
    const meta = await metaOf(repo, work.oid, metas);
    const mine = await blobAt(repo, work.oid, work.path);
    if (!mine) throw gitErr("A file vanished while it was blamed.");
    const newText = textOf(mine.blob);
    let remaining = work.lines;
    for (const parent of meta.parents) {
      if (remaining.length === 0) break;
      let parentPath = work.path;
      let entry = await blobAt(repo, parent, work.path);
      if (!entry) {
        const from = await renamedFrom(repo, parent, work.oid, mine.oid, work.path, true);
        if (from) {
          entry = await blobAt(repo, parent, from);
          parentPath = from;
        }
      }
      if (!entry) continue;
      const time = (await metaOf(repo, parent, metas)).time;
      if (entry.oid === mine.oid) {
        push(parent, parentPath, time, remaining);
        remaining = [];
        break;
      }
      if (newText === null || textOf(entry.blob) === null) continue;
      const map = unchanged(core.textDiff(work.path, entry.blob, mine.blob), lineCount(newText));
      if (!map) continue;
      const pass: Line[] = [];
      const keep: Line[] = [];
      for (const [fin, cur] of remaining) {
        const old = map[cur] ?? -1;
        if (old >= 0) pass.push([fin, old]);
        else keep.push([fin, cur]);
      }
      remaining = keep;
      if (pass.length > 0) push(parent, parentPath, time, pass);
    }
    for (const [fin, cur] of remaining) blamed[fin] = [work.oid, work.path, cur];
  }

  let run: { oid: string; path: string; orig: number; len: number } | null = null;
  let start = 1;
  const flush = async () => {
    if (!run) return;
    const hunk: GitBlameHunk = {
      start,
      lines: run.len,
      commit: (await metaOf(repo, run.oid, metas)).commit,
      path: `/${run.path}`,
      orig_start: run.orig + 1,
    };
    out.hunks.push(hunk);
    start += run.len;
    run = null;
  };
  for (const b of blamed) {
    if (!b) throw gitErr("A line was not blamed.");
    const [oid, p, cur] = b;
    if (run && run.oid === oid && run.path === p && run.orig + run.len === cur) run.len++;
    else {
      await flush();
      run = { oid, path: p, orig: cur, len: 1 };
    }
  }
  await flush();
  return out;
}

// --- search ---------------------------------------------------------------------------------

interface Changed {
  path: string;
  old: string | null;
  neu: string | null;
}

interface TreeEntry {
  mode: string;
  oid: string;
  type: string;
}

async function treeMap(repo: Repo, oid: string): Promise<Map<string, TreeEntry>> {
  const { tree } = await git.readTree({ ...base(repo), oid });
  return new Map(tree.map((e) => [e.path, { mode: e.mode, oid: e.oid, type: e.type }]));
}

/** The files that differ between two trees below `want` (a prefix with a leading `/`). */
async function changedFiles(repo: Repo, prefix: string, old: string | null, neu: string | null, want: string, out: Changed[]): Promise<void> {
  const om = old ? await treeMap(repo, old) : new Map<string, TreeEntry>();
  const nm = neu ? await treeMap(repo, neu) : new Map<string, TreeEntry>();
  const names = [...new Set([...om.keys(), ...nm.keys()])].sort();
  for (const name of names) {
    const o = om.get(name);
    const n = nm.get(name);
    if (o && n && o.oid === n.oid && o.mode === n.mode) continue;
    const full = `${prefix}${name}`;
    const oldFile = o?.type === "blob" ? o.oid : null;
    const newFile = n?.type === "blob" ? n.oid : null;
    if ((oldFile || newFile) && `/${full}`.startsWith(want)) out.push({ path: full, old: oldFile, neu: newFile });
    const oldTree = o?.type === "tree" ? o.oid : null;
    const newTree = n?.type === "tree" ? n.oid : null;
    if (oldTree || newTree) {
      const dir = `/${full}/`;
      if (dir.startsWith(want) || want.startsWith(dir)) await changedFiles(repo, `${full}/`, oldTree, newTree, want, out);
    }
  }
}

async function needleLines(repo: Repo, oid: string | null, needle: string): Promise<Map<string, number> | null> {
  const lines = new Map<string, number>();
  if (!oid) return lines;
  const { blob } = await git.readBlob({ ...base(repo), oid });
  if (blob.length > FILE_MAX_BYTES) return null;
  const text = textOf(blob);
  if (text === null) return null;
  for (const line of text.split(/\r?\n/)) {
    const lower = line.toLowerCase();
    if (lower.includes(needle)) lines.set(lower, (lines.get(lower) ?? 0) + 1);
  }
  return lines;
}

/** The files of a commit in which a line with `needle` was added or removed; none when the
 *  commit only moved such lines between its files. */
async function needlePaths(repo: Repo, changed: Changed[], needle: string): Promise<string[]> {
  const total = new Map<string, number>();
  const paths: string[] = [];
  for (const c of changed.slice(0, FILES_PER_COMMIT)) {
    const old = await needleLines(repo, c.old, needle);
    const neu = await needleLines(repo, c.neu, needle);
    if (!old || !neu) continue;
    const delta = new Map<string, number>();
    for (const [l, n] of neu) delta.set(l, (delta.get(l) ?? 0) + n);
    for (const [l, n] of old) delta.set(l, (delta.get(l) ?? 0) - n);
    let any = false;
    for (const [l, n] of delta) {
      if (n === 0) continue;
      any = true;
      total.set(l, (total.get(l) ?? 0) + n);
    }
    if (any) paths.push(`/${c.path}`);
  }
  if ([...total.values()].every((n) => n === 0)) return [];
  return paths.slice(0, HIT_PATHS);
}

async function searchHistory(ctx: WebGitContext, query: GitHistoryQuery): Promise<GitHistoryPage> {
  const repo = ctx.repo();
  const all = await graphPage(repo, { skip: 0, limit: Number.MAX_SAFE_INTEGER, ...(query.refs ? { refs: query.refs } : {}) });
  const lower = (s: string | null | undefined) => (s ? s.toLowerCase() : "");
  const message = lower(query.message);
  const author = lower(query.author);
  const content = lower(query.content);
  const want = query.path ? (query.path.startsWith("/") ? query.path : `/${query.path}`) : "";
  const skip = query.skip ?? 0;
  const page: GitHistoryPage = { hits: [], more: false, scanned: 0 };
  let seen = 0;
  let examined = 0;
  for (const row of all.commits) {
    page.scanned++;
    if (content && seen >= skip) {
      if (examined >= HISTORY_SCAN_LIMIT) {
        page.more = true;
        break;
      }
      examined++;
    }
    if ((query.since != null && row.time < query.since) || (query.until != null && row.time > query.until)) continue;
    const matched: GitHistoryField[] = [];
    let tree: string | null = null;
    if (message) {
      const { commit } = await git.readCommit({ ...base(repo), oid: row.oid });
      tree = commit.tree;
      if (!commit.message.toLowerCase().includes(message)) continue;
      matched.push("message");
    }
    if (author) {
      if (!`${row.author} <${row.email}>`.toLowerCase().includes(author)) continue;
      matched.push("author");
    }
    let paths: string[] = [];
    if (want || content) {
      if (row.parents.length > 1) continue;
      tree ??= (await git.readCommit({ ...base(repo), oid: row.oid })).commit.tree;
      const parentTree = row.parents[0] ? (await git.readCommit({ ...base(repo), oid: row.parents[0] })).commit.tree : null;
      const changed: Changed[] = [];
      await changedFiles(repo, "", parentTree, tree, want || "/", changed);
      if (changed.length === 0) continue;
      if (want) matched.push("path");
      if (content) {
        const hit = await needlePaths(repo, changed, content);
        if (hit.length === 0) continue;
        matched.push("content");
        paths = hit;
      } else paths = changed.slice(0, HIT_PATHS).map((c) => `/${c.path}`);
    }
    seen++;
    if (seen <= skip) continue;
    if (page.hits.length >= query.limit) {
      page.more = true;
      break;
    }
    const hit: GitHistoryHit = { commit: row, matched, ...(paths.length > 0 ? { paths } : {}) };
    page.hits.push(hit);
  }
  return page;
}

// --- reflog ---------------------------------------------------------------------------------

const NULL_OID = "0".repeat(40);

async function reflog(ctx: WebGitContext, name: string, limit: number): Promise<GitReflogEntry[]> {
  const repo = ctx.repo();
  const short = name.trim();
  if (!short) throw gitErr("No ref given.");
  const full = short === "HEAD" || short.startsWith("refs/") ? short : `refs/heads/${short}`;
  if (full.split("/").some((s) => s === ".." || s === "." || s === "")) throw gitErr(`Invalid ref "${name}".`);
  let text: string;
  try {
    const bytes = (await repo.fs.promises.readFile(`${repo.dir.replace(/\/+$/, "")}/.git/logs/${full}`)) as Uint8Array;
    text = new TextDecoder().decode(bytes);
  } catch (e) {
    const code = (e as { code?: string }).code;
    if (code === "ENOENT" || code === "ENOTDIR" || code === "EISDIR") return [];
    throw e;
  }
  const lines = text.split("\n").filter((l) => l.length > 0);
  const out: GitReflogEntry[] = [];
  for (let i = lines.length - 1, index = 0; i >= 0 && index < limit; i--, index++) {
    const line = lines[i];
    const tab = line.indexOf("\t");
    const head = tab >= 0 ? line.slice(0, tab) : line;
    const message = tab >= 0 ? line.slice(tab + 1).trim() : "";
    const old = head.slice(0, 40);
    const neu = head.slice(41, 81);
    // `<name> <<email>> <time> <tz>`: the time is the second to last field.
    const sig = head.slice(82).trim().split(" ");
    const time = Number(sig[sig.length - 2]) || 0;
    let commit: GitCommit | undefined;
    try {
      const { commit: c } = await git.readCommit({ ...base(repo), oid: neu });
      commit = { oid: neu, summary: c.message.split("\n")[0].trim(), author: c.author.name, time: c.author.timestamp };
    } catch {
      commit = undefined;
    }
    out.push({ name: full, index, old: old === NULL_OID ? null : old, new: neu, message, time, ...(commit ? { commit } : {}) });
  }
  return out;
}

export function webGitHistory(ctx: WebGitContext): GitHistoryMethods {
  return {
    blame: (rev, path) => ctx.run(() => blame(ctx, rev, path)),
    searchHistory: (query) => ctx.run(() => searchHistory(ctx, query)),
    reflog: (name, limit) => ctx.run(() => reflog(ctx, name, limit)),
  };
}
