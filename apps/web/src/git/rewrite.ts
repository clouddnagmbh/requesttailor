// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Secret rescue on isomorphic-git (wave 12, strand G): the history scan and the rewrite, the
// same steps as the desktop's `crates/rt-git/src/rewrite.rs`. What is found and how a text is
// rewritten comes from the core (`crates/rt-wasm/src/rescue_js.rs`: `HistoryScanner`,
// `rescueFindingRefs`, `RescueRewriter`), so both shells find and write the same; this file
// only walks and writes Git objects. Independent of the browser (Node's fs in the tests).
//
// - Scan: every commit reachable from `HEAD` and the local branches, each blob once.
//   Backups (`refs/requesttailor/backup/…`), remote-tracking refs and tags are not scanned.
// - Rewrite: refuses during a merge, on a detached `HEAD` and with uncommitted changes (the
//   caller refuses unsaved ones). New blobs, trees and commits are written byte for byte from
//   the old ones with only the changed ids (commit signatures dropped), so untouched objects
//   keep their ids and the trees equal the desktop's. Then the value goes to `store`, backups
//   of the old tips are written, the branches move and the work tree follows `HEAD`.
//   No push; tags stay (the result names those on rewritten commits).

import "./buffer-global";

import git from "isomorphic-git";

import {
  BackendError,
  type GitCommit,
  type HistoryScan,
  type RescuePlan,
  type RescueResult,
  type RewrittenBranch,
  type ScanProgress,
} from "@requesttailor/ui";

import type { Repo } from "./repo";
import { status } from "./repo";
import { textOf } from "./scan";

/** `rt_app::rescue::BACKUP_PREFIX`. */
export const BACKUP_PREFIX = "refs/requesttailor/backup/";
/** Progress is reported after this many commits (and after the last), as natively. */
const PROGRESS_EVERY = 20;

// ---- the core's part (rt-wasm `rescue_js.rs`) ----

export interface CoreHistoryScanner {
  knows(id: string): boolean;
  addBlob(id: string, path: string, text?: string | null): void;
  addCommit(commit: GitCommit, parents: string[], files: [string, string][]): void;
  finish(branches: [string, string][], head?: string | null): HistoryScan;
  free?(): void;
}

export interface CoreRescueRewriter {
  /** The text rewritten, `undefined` when it does not hold the value. */
  rewrite(text: string): string | undefined;
  /** For the secret store only. */
  value(): string;
  free?(): void;
}

export interface RescueCore {
  HistoryScanner: new () => CoreHistoryScanner;
  RescueRewriter: new (plan: RescuePlan, texts: [string, string, string | null][]) => CoreRescueRewriter;
  rescueFindingRefs(plan: RescuePlan): { commit: string; path: string }[];
}

// ---- helpers ----

const gitErr = (message: string) => new BackendError("git", message);

function base(repo: Repo) {
  return { fs: repo.fs, dir: repo.dir, cache: repo.cache };
}

function toCommit(oid: string, c: { message: string; author: { name: string; timestamp: number } }): GitCommit {
  return { oid, summary: c.message.split("\n")[0], author: c.author.name, time: c.author.timestamp };
}

async function refOid(repo: Repo, ref: string): Promise<string | null> {
  try {
    return await git.resolveRef({ ...base(repo), ref });
  } catch {
    return null;
  }
}

async function exists(repo: Repo, rel: string): Promise<boolean> {
  try {
    await repo.fs.promises.stat(`${repo.dir.replace(/\/+$/, "")}/${rel}`);
    return true;
  } catch {
    return false;
  }
}

/** Local branches `[name, tip]`, sorted by name. */
async function branchTips(repo: Repo): Promise<[string, string][]> {
  const names = await git.listBranches({ ...base(repo) });
  const out: [string, string][] = [];
  for (const name of names.sort()) {
    const oid = await refOid(repo, `refs/heads/${name}`);
    if (oid) out.push([name, oid]);
  }
  return out;
}

interface Node {
  parents: string[];
  time: number;
  commit: GitCommit;
  tree: string;
}

/** Every commit reachable from `tips`. */
async function graph(repo: Repo, tips: string[]): Promise<Map<string, Node>> {
  const out = new Map<string, Node>();
  const stack = [...tips];
  while (stack.length > 0) {
    const oid = stack.pop() as string;
    if (out.has(oid)) continue;
    const { commit } = await git.readCommit({ ...base(repo), oid });
    out.set(oid, { parents: commit.parent, time: commit.author.timestamp, commit: toCommit(oid, commit), tree: commit.tree });
    stack.push(...commit.parent);
  }
  return out;
}

/** Commits parents first; among those ready, the oldest (author time, then id) first. */
function topoOrder(nodes: Map<string, Node>): string[] {
  const waiting = new Map<string, number>();
  const children = new Map<string, string[]>();
  for (const [oid, n] of nodes) {
    const parents = [...new Set(n.parents.filter((p) => nodes.has(p)))];
    waiting.set(oid, parents.length);
    for (const p of parents) children.set(p, [...(children.get(p) ?? []), oid]);
  }
  const key = (oid: string): [number, string] => [nodes.get(oid)?.time ?? 0, oid];
  const less = (a: [number, string], b: [number, string]) => a[0] < b[0] || (a[0] === b[0] && a[1] < b[1]);
  const ready: [number, string][] = [...waiting].filter(([, n]) => n === 0).map(([oid]) => key(oid));
  const out: string[] = [];
  while (ready.length > 0) {
    let best = 0;
    for (let i = 1; i < ready.length; i++) if (less(ready[i], ready[best])) best = i;
    const [, oid] = ready.splice(best, 1)[0];
    out.push(oid);
    for (const child of children.get(oid) ?? []) {
      const n = (waiting.get(child) ?? 1) - 1;
      waiting.set(child, n);
      if (n === 0) ready.push(key(child));
    }
  }
  return out;
}

/** Blobs (`100644`/`100755`) of a tree, `[path, oid]`, memoized per tree. */
async function blobsOf(repo: Repo, tree: string, memo: Map<string, [string, string][]>): Promise<[string, string][]> {
  const known = memo.get(tree);
  if (known) return known;
  const out: [string, string][] = [];
  const { tree: entries } = await git.readTree({ ...base(repo), oid: tree });
  for (const e of entries) {
    if (e.type === "tree") {
      for (const [p, oid] of await blobsOf(repo, e.oid, memo)) out.push([`${e.path}/${p}`, oid]);
    } else if (e.type === "blob" && (e.mode === "100644" || e.mode === "100755")) {
      out.push([e.path, e.oid]);
    }
  }
  memo.set(tree, out);
  return out;
}

async function rawObject(repo: Repo, oid: string): Promise<Uint8Array> {
  const { object } = await git.readObject({ ...base(repo), oid, format: "content" });
  return object as Uint8Array;
}

async function writeRaw(repo: Repo, type: "blob" | "tree" | "commit", object: Uint8Array): Promise<string> {
  return git.writeObject({ fs: repo.fs, dir: repo.dir, type, object, format: "content" });
}

const enc = new TextEncoder();
const dec = new TextDecoder();

function hex(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += b.toString(16).padStart(2, "0");
  return s;
}

function unhex(s: string): Uint8Array {
  const out = new Uint8Array(s.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(s.slice(i * 2, i * 2 + 2), 16);
  return out;
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

function equal(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((x, i) => x === b[i]);
}

/** A raw commit with `tree` and `parents` in place of its own, every other header line
 *  (continuation lines included) and the message as they are; signatures dropped. */
export function recommit(data: Uint8Array, tree: string, parents: string[]): Uint8Array {
  let end = -1;
  for (let i = 0; i + 1 < data.length; i++) {
    if (data[i] === 10 && data[i + 1] === 10) {
      end = i;
      break;
    }
  }
  if (end < 0) throw gitErr("Cannot read a commit: unexpected format.");
  const head = data.subarray(0, end + 1);
  const message = data.subarray(end + 1);
  const parts: Uint8Array[] = [enc.encode(`tree ${tree}\n`), ...parents.map((p) => enc.encode(`parent ${p}\n`))];
  let skip = false;
  let at = 0;
  while (at < head.length) {
    let next = head.indexOf(10, at);
    next = next < 0 ? head.length : next + 1;
    const line = head.subarray(at, next);
    if (line[0] !== 32) {
      const space = line.indexOf(32);
      const name = dec.decode(line.subarray(0, space < 0 ? line.length : space));
      skip = name === "tree" || name === "parent" || name === "gpgsig" || name === "gpgsig-sha256";
    }
    if (!skip) parts.push(line);
    at = next;
  }
  parts.push(message);
  return concat(parts);
}

// ---- scan ----

/** The history scan (see the file comment). */
export async function scanHistory(
  repo: Repo,
  core: RescueCore,
  onProgress?: (progress: ScanProgress) => void,
): Promise<HistoryScan> {
  const branches = await branchTips(repo);
  const head = await refOid(repo, "HEAD");
  const tips = [...new Set([...branches.map(([, oid]) => oid), ...(head ? [head] : [])])];
  const nodes = await graph(repo, tips);
  const total = nodes.size;
  onProgress?.({ scanned: 0, total });
  const scanner = new core.HistoryScanner();
  try {
    const memo = new Map<string, [string, string][]>();
    let scanned = 0;
    for (const oid of [...nodes.keys()].sort()) {
      const node = nodes.get(oid) as Node;
      const files = await blobsOf(repo, node.tree, memo);
      for (const [path, blob] of files) {
        if (scanner.knows(blob)) continue;
        const { blob: bytes } = await git.readBlob({ ...base(repo), oid: blob });
        scanner.addBlob(blob, path, textOf(bytes));
      }
      scanner.addCommit(node.commit, node.parents, files);
      scanned++;
      if (scanned % PROGRESS_EVERY === 0 || scanned === total) onProgress?.({ scanned, total });
    }
    return scanner.finish(branches, head);
  } finally {
    scanner.free?.();
  }
}

// ---- rewrite ----

async function textAt(repo: Repo, commit: string, path: string): Promise<string | null> {
  try {
    const { blob } = await git.readBlob({ ...base(repo), oid: commit, filepath: path });
    return textOf(blob);
  } catch {
    return null;
  }
}

/** The upstream remote of local branch `name`, if the branch was pushed or fetched. */
async function pushedTo(repo: Repo, name: string): Promise<string | null> {
  const remote = (await git.getConfig({ ...base(repo), path: `branch.${name}.remote` })) as string | undefined;
  const merge = (await git.getConfig({ ...base(repo), path: `branch.${name}.merge` })) as string | undefined;
  if (!remote || !merge || remote === "." || !merge.startsWith("refs/heads/")) return null;
  const tracking = `refs/remotes/${remote}/${merge.slice("refs/heads/".length)}`;
  return (await refOid(repo, tracking)) ? remote : null;
}

/** Tags that point at a commit `map` rewrote. */
async function movedTags(repo: Repo, map: Map<string, string>): Promise<string[]> {
  const out: string[] = [];
  for (const tag of await git.listTags({ ...base(repo) })) {
    let oid = await refOid(repo, `refs/tags/${tag}`);
    if (!oid) continue;
    try {
      const { type } = await git.readObject({ ...base(repo), oid });
      if (type === "tag") oid = (await git.readTag({ ...base(repo), oid })).tag.object;
    } catch {
      continue;
    }
    const moved = map.get(oid);
    if (moved && moved !== oid) out.push(tag);
  }
  return out.sort();
}

/** Rewrites the local branches as `plan` says (after the user confirmed); `store` gets the
 *  value before any ref changes; `now` (Unix seconds) names the backups. */
export async function rewriteHistory(
  repo: Repo,
  core: RescueCore,
  plan: RescuePlan,
  now: number,
  store: (value: string) => Promise<void>,
): Promise<RescueResult> {
  const refs = core.rescueFindingRefs(plan);
  if (await exists(repo, ".git/MERGE_HEAD")) throw gitErr("Finish the merge before rewriting the history.");
  const current = await git.currentBranch({ ...base(repo), fullname: false });
  if (!current) throw gitErr("Switch to a branch before rewriting the history.");
  const changedFile = (await status(repo)).files.find((f) => f.state !== "untracked");
  if (changedFile) {
    throw gitErr(`Commit or discard your changes before rewriting the history (${changedFile.path}).`);
  }
  const texts: [string, string, string | null][] = [];
  for (const r of refs) texts.push([r.commit, r.path, await textAt(repo, r.commit, r.path)]);
  const rewriter = new core.RescueRewriter(plan, texts);
  try {
    // New objects first; nothing points at them yet.
    const branches = await branchTips(repo);
    const nodes = await graph(
      repo,
      branches.map(([, oid]) => oid),
    );
    const blobs = new Map<string, string>();
    const trees = new Map<string, string>();
    const blob = async (oid: string): Promise<string> => {
      const known = blobs.get(oid);
      if (known) return known;
      const { blob: bytes } = await git.readBlob({ ...base(repo), oid });
      const text = textOf(bytes);
      const replaced = text === null ? undefined : rewriter.rewrite(text);
      const out = replaced === undefined ? oid : await writeRaw(repo, "blob", enc.encode(replaced));
      blobs.set(oid, out);
      return out;
    };
    const tree = async (oid: string): Promise<string> => {
      const known = trees.get(oid);
      if (known) return known;
      const data = await rawObject(repo, oid);
      const parts: Uint8Array[] = [];
      let changed = false;
      let at = 0;
      while (at < data.length) {
        const space = data.indexOf(32, at);
        const nul = data.indexOf(0, space);
        if (space < 0 || nul < 0 || nul + 21 > data.length) throw gitErr("Cannot read a tree: unexpected format.");
        const mode = dec.decode(data.subarray(at, space));
        const old = hex(data.subarray(nul + 1, nul + 21));
        const next =
          mode === "40000" || mode === "040000" ? await tree(old) : mode.startsWith("100") ? await blob(old) : old;
        changed ||= next !== old;
        parts.push(data.subarray(at, nul + 1), unhex(next));
        at = nul + 21;
      }
      const out = changed ? await writeRaw(repo, "tree", concat(parts)) : oid;
      trees.set(oid, out);
      return out;
    };
    const map = new Map<string, string>();
    let rewritten = 0;
    for (const oid of topoOrder(nodes)) {
      const node = nodes.get(oid) as Node;
      const newTree = await tree(node.tree);
      const parents = node.parents.map((p) => map.get(p) ?? p);
      let next = oid;
      if (newTree !== node.tree || parents.some((p, i) => p !== node.parents[i])) {
        const data = await rawObject(repo, oid);
        const out = recommit(data, newTree, parents);
        next = equal(out, data) ? oid : await writeRaw(repo, "commit", out);
      }
      if (next !== oid) rewritten++;
      map.set(oid, next);
    }
    const changed = branches
      .map(([name, old]) => [name, old, map.get(old) ?? old] as const)
      .filter(([, old, next]) => next !== old);
    if (changed.length === 0) {
      throw gitErr("The value is not in the history of any local branch: scan the history again.");
    }

    await store(rewriter.value());

    // Backups, under a prefix no earlier rescue used.
    let unix = now;
    const taken = async (u: number) => {
      for (const [name] of changed) if (await refOid(repo, `${BACKUP_PREFIX}${u}/${name}`)) return true;
      return false;
    };
    while (await taken(unix)) unix++;
    const backup = `${BACKUP_PREFIX}${unix}/`;
    for (const [name, old] of changed) {
      await git.writeRef({ ...base(repo), ref: `${backup}${name}`, value: old, force: false });
    }
    for (const [name, , next] of changed) {
      await git.writeRef({ ...base(repo), ref: `refs/heads/${name}`, value: next, force: true });
    }
    // The work tree follows HEAD (it was clean).
    if (changed.some(([name]) => name === current)) {
      await git.checkout({ ...base(repo), ref: current, force: true });
    }

    const out: RewrittenBranch[] = [];
    for (const [name, old, next] of changed) {
      const remote = await pushedTo(repo, name);
      out.push({ name, old, new: next, pushed: remote !== null, remote });
    }
    return { rewritten, branches: out, backup, tags: await movedTags(repo, map) };
  } finally {
    rewriter.free?.();
  }
}
