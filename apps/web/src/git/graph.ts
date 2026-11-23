// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand F: the commit graph of the web app (`GitApi.graph`, `refs`, `changes`,
// `fileDiff`) with isomorphic-git over the repository of the active collection, with the same
// answers as the desktop (`rt-git/src/graph.rs`): all refs, date order (children before
// parents, ties newest first, `@requesttailor/ui`'s `dateOrder`), renames by content or line
// similarity, binary files without line diffs. In the browser the repository root is the
// collection folder: every collection file (`treePath`) outside a nested collection belongs to
// the active collection.
//
// isomorphic-git loads with the first call, like ./revisions.ts. Commits are immutable, so
// what was read once is kept by id (`commitCache`) and the next page is quick.

import {
  dateOrder,
  graphPage,
  reachable,
  sortChanges,
  sortRefs,
  BackendError,
  type GitChangedFile,
  type GitDiff,
  type GitGraphMethods,
  type GitGraphPage,
  type GitGraphQuery,
  type GitRef,
} from "@requesttailor/ui";

import type { WebHost } from "../web-backend";
import type { Repo } from "./repo";

/** What `index.ts` hands the wave-14 Git parts: the host, the repository of the active
 *  collection and the queue that runs one Git operation at a time (errors mapped). */
export interface WebGitContext {
  host: WebHost;
  repo(): Repo;
  run<T>(op: () => Promise<T>): Promise<T>;
}

type Git = typeof import("isomorphic-git").default;
type Revisions = typeof import("./revisions");

let loading: Promise<[Git, Revisions]> | null = null;
function iso(): Promise<[Git, Revisions]> {
  loading ??= (async (): Promise<[Git, Revisions]> => {
    await import("./buffer-global");
    return [(await import("isomorphic-git")).default, await import("./revisions")];
  })().catch((e: unknown) => {
    loading = null;
    throw e;
  });
  return loading;
}

const gitErr = (message: string) => new BackendError("git", message);
const COLLECTION_FILE = "opencollection.yml";
/** Bytes looked at to tell binary from text, as natively. */
const SNIFF = 8000;
const RENAME_MAX_BYTES = 512 * 1024;
const RENAME_MAX_PAIRS = 10_000;
const SIMILARITY = 0.5;

function base(repo: Repo) {
  return { fs: repo.fs, dir: repo.dir, cache: repo.cache };
}

interface CommitInfo {
  oid: string;
  parents: string[];
  tree: string;
  summary: string;
  author: string;
  email: string;
  authorTime: number;
  /** Committer time: the order. */
  time: number;
}

/** Commits by id; ids are content hashes, so one cache serves every repository. */
const commitCache = new Map<string, CommitInfo>();

async function commitOf(git: Git, repo: Repo, oid: string): Promise<CommitInfo> {
  const hit = commitCache.get(oid);
  if (hit) return hit;
  const { commit } = await git.readCommit({ ...base(repo), oid });
  const info: CommitInfo = {
    oid,
    parents: commit.parent,
    tree: commit.tree,
    summary: commit.message.split("\n")[0].trim(),
    author: commit.author.name,
    email: commit.author.email,
    authorTime: commit.author.timestamp,
    time: commit.committer.timestamp,
  };
  commitCache.set(oid, info);
  return info;
}

/** The commit the full ref `ref` leads to, and an annotated tag's first message line. */
async function peel(git: Git, repo: Repo, ref: string): Promise<{ commit: string; message: string | null } | null> {
  let oid: string;
  try {
    oid = await git.resolveRef({ ...base(repo), ref });
  } catch {
    return null;
  }
  let message: string | null = null;
  let first = true;
  for (let depth = 0; depth < 10; depth++) {
    let type: string;
    try {
      type = (await git.readObject({ ...base(repo), oid, format: "parsed" })).type;
    } catch {
      return null;
    }
    if (type === "commit") return { commit: oid, message };
    if (type !== "tag") return null;
    const t = (await git.readTag({ ...base(repo), oid })).tag;
    if (first) message = t.message.split("\n")[0].trim() || null;
    first = false;
    oid = t.object;
  }
  return null;
}

async function headState(git: Git, repo: Repo): Promise<{ branch: string | null; head: string | null }> {
  const branch = (await git.currentBranch({ ...base(repo), fullname: true }).catch(() => undefined)) ?? null;
  const head = await git.resolveRef({ ...base(repo), ref: "HEAD" }).catch(() => null);
  return { branch, head };
}

/** Every commit reachable from `tips`. */
async function walkAll(git: Git, repo: Repo, tips: string[]): Promise<CommitInfo[]> {
  const out: CommitInfo[] = [];
  const seen = new Set<string>();
  const todo = [...tips];
  while (todo.length > 0) {
    const oid = todo.pop()!;
    if (seen.has(oid)) continue;
    seen.add(oid);
    const c = await commitOf(git, repo, oid).catch(() => null);
    if (!c) continue; // Missing (a shallow clone): the line ends here.
    out.push(c);
    for (const p of c.parents) if (!seen.has(p)) todo.push(p);
  }
  return out;
}

function reach(all: Map<string, CommitInfo>, from: string): Set<string> {
  const seen = new Set<string>();
  const todo = [from];
  while (todo.length > 0) {
    const o = todo.pop()!;
    if (seen.has(o)) continue;
    seen.add(o);
    todo.push(...(all.get(o)?.parents ?? []));
  }
  return seen;
}

function countOnly(all: Map<string, CommitInfo>, tip: string, hidden: string): number {
  const hide = reach(all, hidden);
  let n = 0;
  for (const o of reach(all, tip)) if (!hide.has(o)) n++;
  return n;
}

/** All refs, as natively: HEAD (detached only), local, remote (no `<remote>/HEAD`), tags. */
export async function listRefs(repo: Repo): Promise<GitRef[]> {
  const [git] = await iso();
  const { branch, head } = await headState(git, repo);
  const refs: GitRef[] = [];
  const add = async (full: string, kind: GitRef["kind"], name: string) => {
    const peeled = await peel(git, repo, full);
    if (!peeled) return;
    refs.push({
      full,
      name,
      kind,
      target: peeled.commit,
      current: full === branch,
      ahead: 0,
      behind: 0,
      ...(kind === "tag" && peeled.message ? { message: peeled.message } : {}),
    });
  };
  for (const b of await git.listBranches(base(repo))) await add(`refs/heads/${b}`, "local", b);
  for (const { remote } of await git.listRemotes(base(repo))) {
    for (const b of await git.listBranches({ ...base(repo), remote })) {
      if (b === "HEAD") continue;
      await add(`refs/remotes/${remote}/${b}`, "remote", `${remote}/${b}`);
    }
  }
  for (const t of await git.listTags(base(repo))) await add(`refs/tags/${t}`, "tag", t);
  // Upstreams and ahead/behind.
  const ups: [GitRef, string][] = [];
  for (const r of refs) {
    if (r.kind !== "local") continue;
    const remote: unknown = await git.getConfig({ ...base(repo), path: `branch.${r.name}.remote` }).catch(() => undefined);
    const merge: unknown = await git.getConfig({ ...base(repo), path: `branch.${r.name}.merge` }).catch(() => undefined);
    if (typeof remote !== "string" || typeof merge !== "string" || remote === "." || !merge.startsWith("refs/heads/")) continue;
    const short = merge.slice("refs/heads/".length);
    r.upstream = `${remote}/${short}`;
    const tracking = refs.find((x) => x.full === `refs/remotes/${remote}/${short}`);
    if (tracking) ups.push([r, tracking.target]);
  }
  if (ups.length > 0) {
    const all = new Map((await walkAll(git, repo, ups.flatMap(([r, t]) => [r.target, t]))).map((c) => [c.oid, c]));
    for (const [r, up] of ups) {
      r.ahead = countOnly(all, r.target, up);
      r.behind = countOnly(all, up, r.target);
    }
  }
  if (branch === null && head) refs.push({ full: "HEAD", name: "HEAD", kind: "head", target: head, current: true, ahead: 0, behind: 0 });
  return sortRefs(refs);
}

/** A page of the graph (date order over all refs, or over the refs asked for). */
export async function graph(repo: Repo, query: GitGraphQuery): Promise<GitGraphPage> {
  const [git] = await iso();
  const refs = await listRefs(repo);
  const { branch, head } = await headState(git, repo);
  let tips: string[];
  if (query.refs) {
    tips = [];
    for (const r of query.refs) {
      const peeled = r === "HEAD" ? (head ? { commit: head } : null) : await peel(git, repo, r);
      if (!peeled) throw gitErr(`Unknown revision "${r}".`);
      tips.push(peeled.commit);
    }
  } else {
    tips = [...new Set([...refs.map((r) => r.target), ...(head ? [head] : [])])];
  }
  const all = await walkAll(git, repo, tips);
  const order = dateOrder(query.refs ? reachable(all, tips) : all);
  return graphPage({ commits: order, refs, head, detached: head !== null && branch === null }, query);
}

interface Entry {
  mode: string;
  oid: string;
  type: string;
}

/** Path → entry of every file of a commit's tree (`null`: none, before a root commit). */
async function flat(git: Git, repo: Repo, commit: string | null): Promise<Map<string, Entry>> {
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
  await walk((await commitOf(git, repo, commit)).tree, "");
  return out;
}

const utf8 = new TextDecoder("utf-8", { fatal: true });

/** Binary for display: a NUL byte near the start, or not UTF-8 (as natively). */
export function isBinary(bytes: Uint8Array): boolean {
  if (bytes.subarray(0, SNIFF).includes(0)) return true;
  try {
    utf8.decode(bytes);
    return false;
  } catch {
    return true;
  }
}

async function blob(git: Git, repo: Repo, oid: string): Promise<Uint8Array> {
  return (await git.readBlob({ ...base(repo), oid })).blob;
}

/** Lines in common over the lines of the larger file (`rt-git/src/file_log.rs`). */
export function similarity(a: string, b: string): number {
  const lines = (t: string) => (t === "" ? [] : t.replace(/\n$/, "").split("\n"));
  const la = lines(a);
  const lb = lines(b);
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

/** Files changed from `from` (`null`: `to`'s first parent) to `to`, as natively; collection
 *  files (outside nested collections) name `collection`. */
export async function changes(repo: Repo, from: string | null, to: string, collection: string | null): Promise<GitChangedFile[]> {
  const [git, rev] = await iso();
  const target = await rev.revParse(repo, to);
  const baseOid = from ? await rev.revParse(repo, from) : ((await commitOf(git, repo, target)).parents[0] ?? null);
  const [old, now] = [await flat(git, repo, baseOid), await flat(git, repo, target)];
  const binary = async (e: Entry) => e.type === "commit" || isBinary(await blob(git, repo, e.oid));
  const out = new Map<string, GitChangedFile>();
  const added: [string, Entry][] = [];
  const deleted: [string, Entry][] = [];
  for (const [p, e] of now) {
    const before = old.get(p);
    if (!before) added.push([p, e]);
    else if (before.oid !== e.oid || before.mode !== e.mode) {
      out.set(p, { path: `/${p}`, change: "modified", binary: (await binary(before)) || (await binary(e)) });
    }
  }
  for (const [p, e] of old) if (!now.has(p)) deleted.push([p, e]);
  // Renames: the same content first, then the most similar text.
  const renamed = new Map<string, string>();
  const taken = new Set<string>();
  for (const [p, e] of added) {
    const hit = deleted.find(([q, d]) => d.oid === e.oid && d.type !== "commit" && !taken.has(q));
    if (hit) {
      renamed.set(p, hit[0]);
      taken.add(hit[0]);
    }
  }
  const openAdded = added.filter(([p]) => !renamed.has(p));
  const openDeleted = deleted.filter(([q]) => !taken.has(q));
  if (openAdded.length > 0 && openDeleted.length > 0 && openAdded.length * openDeleted.length <= RENAME_MAX_PAIRS) {
    const text = async (e: Entry): Promise<string | null> => {
      if (e.type === "commit") return null;
      const bytes = await blob(git, repo, e.oid);
      if (bytes.length > RENAME_MAX_BYTES || isBinary(bytes)) return null;
      return utf8.decode(bytes);
    };
    const olds: [string, string | null][] = [];
    for (const [q, d] of openDeleted) olds.push([q, await text(d)]);
    for (const [p, e] of openAdded) {
      const t = await text(e);
      if (t === null) continue;
      let best: [number, string] | null = null;
      for (const [q, ot] of olds) {
        if (ot === null || taken.has(q)) continue;
        const s = similarity(t, ot);
        if (s >= SIMILARITY && (!best || s > best[0])) best = [s, q];
      }
      if (best) {
        renamed.set(p, best[1]);
        taken.add(best[1]);
      }
    }
  }
  for (const [p, e] of added) {
    const fromPath = renamed.get(p);
    out.set(p, { path: `/${p}`, change: fromPath ? "renamed" : "added", ...(fromPath ? { from: `/${fromPath}` } : {}), binary: await binary(e) });
  }
  for (const [p, e] of deleted) if (!taken.has(p)) out.set(p, { path: `/${p}`, change: "deleted", binary: await binary(e) });
  if (collection) {
    const nested: string[] = [];
    for (const p of [...now.keys(), ...old.keys()]) if (p.endsWith(`/${COLLECTION_FILE}`)) nested.push(p.slice(0, -COLLECTION_FILE.length));
    for (const [p, f] of out) if (rev.treePath(p) && !nested.some((dir) => p.startsWith(dir))) f.collection = collection;
  }
  return sortChanges([...out.values()]);
}

function checkPath(path: string): string {
  const p = path.replace(/^\//, "");
  if (!p || p.includes("\\") || p.split("/").some((s) => s === "" || s === "." || s === ".." || s.toLowerCase() === ".git")) {
    throw gitErr(`Invalid path "${path}".`);
  }
  return p;
}

async function workFile(repo: Repo, rel: string): Promise<Uint8Array | null> {
  try {
    return (await repo.fs.promises.readFile(`${repo.dir.replace(/\/$/, "")}/${rel}`)) as Uint8Array;
  } catch (e) {
    const code = (e as { code?: string }).code;
    if (code === "ENOENT" || code === "ENOTDIR" || code === "EISDIR") return null;
    throw e;
  }
}

/** Both sides of `fileDiff`: `to` `null` is the work tree (against `from` or `HEAD`); a file
 *  that `to` renamed is compared with its old path. */
export async function fileSides(repo: Repo, from: string | null, to: string | null, path: string): Promise<{ path: string; old: Uint8Array | null; new: Uint8Array | null }> {
  const [git, rev] = await iso();
  const rel = checkPath(path);
  let baseOid: string | null;
  let target: string | null = null;
  let now: Uint8Array | null;
  if (to !== null) {
    target = await rev.revParse(repo, to);
    baseOid = from ? await rev.revParse(repo, from) : ((await commitOf(git, repo, target)).parents[0] ?? null);
    const e = (await flat(git, repo, target)).get(rel);
    now = e && e.type !== "commit" ? await blob(git, repo, e.oid) : null;
  } else {
    baseOid = from ? await rev.revParse(repo, from) : await git.resolveRef({ ...base(repo), ref: "HEAD" }).catch(() => null);
    now = await workFile(repo, rel);
  }
  const olds = await flat(git, repo, baseOid);
  let e = olds.get(rel);
  if (!e && now && baseOid && target) {
    const renamed = (await changes(repo, baseOid, target, null)).find((f) => f.path === `/${rel}`)?.from;
    if (renamed) e = olds.get(renamed.slice(1));
  }
  const old = e && e.type !== "commit" ? await blob(git, repo, e.oid) : null;
  return { path: `/${rel}`, old, new: now };
}

export function webGitGraph(ctx: WebGitContext): GitGraphMethods {
  const collection = () => {
    try {
      return ctx.host.session().id;
    } catch {
      return null;
    }
  };
  return {
    graph: (query) => ctx.run(() => graph(ctx.repo(), query)),
    refs: () => ctx.run(() => listRefs(ctx.repo())),
    changes: (from, to) => ctx.run(() => changes(ctx.repo(), from, to, collection())),
    fileDiff: (from, to, path) =>
      ctx.run(async (): Promise<GitDiff> => {
        const sides = await fileSides(ctx.repo(), from, to, path);
        const core = await ctx.host.deps.loadCore();
        const diff = core.textDiff(sides.path, sides.old, sides.new);
        if (!diff.binary && ((sides.old && isBinary(sides.old)) || (sides.new && isBinary(sides.new)))) {
          return { ...diff, binary: true, hunks: [] };
        }
        return diff;
      }),
  };
}
