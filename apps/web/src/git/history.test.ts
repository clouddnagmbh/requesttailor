// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand C: blame, history search, reflog and the history of any file on
// isomorphic-git (Node's fs, a temporary folder), the same repositories and the same answers as
// crates/rt-git/tests/{blame,history_search,reflog}.rs. Blame needs the real core's line diff
// (rt-wasm from `node scripts/build-wasm.mjs`; without it its tests are skipped). The `git`
// command line, when installed, is compared too.

import { execFileSync } from "node:child_process";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import git from "isomorphic-git";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import type { GitHistoryMethods, GitHistoryQuery } from "@requesttailor/ui";

import type { FsClient } from "./fs";
import { followFileLog } from "./file-log";
import { webGitHistory } from "./history";
import type { WebGitContext } from "./graph";
import type { Repo } from "./repo";
import type { WebHost } from "../web-backend";

const WASM_JS = fileURLToPath(new URL("../../public/wasm/rt_wasm.js", import.meta.url));
const WASM_BG = fileURLToPath(new URL("../../public/wasm/rt_wasm_bg.wasm", import.meta.url));
const built = nodeFs.existsSync(WASM_JS) && nodeFs.existsSync(WASM_BG);

const hasGit = (() => {
  try {
    execFileSync("git", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
})();

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

const enc = new TextEncoder();

function newRepo(): Repo {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-history-"));
  dirs.push(dir);
  return { fs: nodeFs as unknown as FsClient, dir, cache: {} };
}

type Files = Record<string, string | Uint8Array>;

async function writeTree(repo: Repo, files: Files): Promise<string> {
  const here: { mode: string; path: string; oid: string; type: "blob" | "tree" }[] = [];
  const below = new Map<string, Files>();
  for (const [p, content] of Object.entries(files)) {
    const [head, ...rest] = p.split("/");
    if (rest.length > 0) {
      const sub = below.get(head) ?? {};
      sub[rest.join("/")] = content;
      below.set(head, sub);
    } else {
      const blob = typeof content === "string" ? enc.encode(content) : content;
      here.push({ mode: "100644", path: head, oid: await git.writeBlob({ fs: repo.fs, dir: repo.dir, blob }), type: "blob" });
    }
  }
  for (const [name, sub] of below) here.push({ mode: "040000", path: name, oid: await writeTree(repo, sub), type: "tree" });
  return git.writeTree({ fs: repo.fs, dir: repo.dir, tree: here });
}

async function snapshot(repo: Repo, parents: string[], files: Files, message: string, time: number, who: string): Promise<string> {
  const sig = { name: who, email: `${who.toLowerCase()}@example.com`, timestamp: time, timezoneOffset: 0 };
  return git.writeCommit({
    fs: repo.fs,
    dir: repo.dir,
    commit: { message: `${message}\n`, tree: await writeTree(repo, files), parent: parents, author: sig, committer: sig },
  });
}

const ref = (repo: Repo, name: string, value: string) => git.writeRef({ fs: repo.fs, dir: repo.dir, ref: name, value, force: true });

function cli(dir: string, ...args: string[]): string {
  return execFileSync("git", ["-C", dir, ...args], { encoding: "utf8", env: { ...process.env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null" } });
}

// The core's line diff, as in the browser (`core.textDiff`).
let core: { textDiff(path: string, old: Uint8Array | null, current: Uint8Array | null): unknown } | null = null;
beforeAll(async () => {
  if (!built) return;
  const mod = (await import(/* @vite-ignore */ pathToFileURL(WASM_JS).href)) as { default(input: unknown): Promise<unknown>; textDiff: unknown };
  await mod.default({ module_or_path: nodeFs.readFileSync(WASM_BG) });
  core = mod as unknown as typeof core;
});

function api(repo: Repo): GitHistoryMethods {
  const host = { deps: { loadCore: async () => core } } as unknown as WebHost;
  const ctx: WebGitContext = { host, repo: () => repo, run: (op) => op() };
  return webGitHistory(ctx);
}

const query = (q: Partial<GitHistoryQuery> = {}): GitHistoryQuery => ({ limit: 100, ...q });

// ---------------------------------------------------------------- blame

interface BlameFixture {
  repo: Repo;
  c1: string;
  c2: string;
  c3: string;
  f1: string;
  m1: string;
  merge: string;
}

/** c1 a b c d (Ada); c2 a B c d e (Grace); c3 renamed to src/lib.txt, D (Linus); f1 x on top
 *  (Ada, feature); m1 E (Grace, main); merge(m1, f1) (Linus). */
async function blameFixture(): Promise<BlameFixture> {
  const repo = newRepo();
  await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
  const readme = "hi\n";
  const c1 = await snapshot(repo, [], { "src/main.txt": "a\nb\nc\nd\n", "README.md": readme }, "one", 1000, "Ada");
  const c2 = await snapshot(repo, [c1], { "src/main.txt": "a\nB\nc\nd\ne\n", "README.md": readme }, "two", 2000, "Grace");
  const c3 = await snapshot(repo, [c2], { "src/lib.txt": "a\nB\nc\nD\ne\n", "README.md": readme }, "three", 3000, "Linus");
  const f1 = await snapshot(repo, [c3], { "src/lib.txt": "x\na\nB\nc\nD\ne\n", "README.md": readme }, "four feature", 4000, "Ada");
  const m1 = await snapshot(repo, [c3], { "src/lib.txt": "a\nB\nc\nD\nE\n", "README.md": readme }, "four main", 4500, "Grace");
  const merge = await snapshot(repo, [m1, f1], { "src/lib.txt": "x\na\nB\nc\nD\nE\n", "README.md": readme }, "merge", 5000, "Linus");
  await ref(repo, "refs/heads/main", merge);
  await ref(repo, "refs/heads/feature", f1);
  return { repo, c1, c2, c3, f1, m1, merge };
}

const perLine = (b: { hunks: { start: number; lines: number; commit: { oid: string } }[] }) =>
  b.hunks.flatMap((h) => Array.from({ length: h.lines }, () => h.commit.oid));

describe.skipIf(!built)("blame", () => {
  it("names the commit of every line across renames and merges", async () => {
    const f = await blameFixture();
    const blame = await api(f.repo).blame("HEAD", "/src/lib.txt");
    expect(blame.path).toBe("/src/lib.txt");
    expect(blame.rev).toBe(f.merge);
    expect(blame.lines).toBe(6);
    expect(blame.binary).toBe(false);
    expect(perLine(blame)).toEqual([f.f1, f.c1, f.c2, f.c1, f.c3, f.m1]);
    let next = 1;
    for (const h of blame.hunks) {
      expect(h.start).toBe(next);
      next += h.lines;
    }
    expect(next).toBe(7);
    const first = blame.hunks.find((h) => h.commit.oid === f.c1)!;
    expect(first.path).toBe("/src/main.txt");
    expect(first.orig_start).toBe(1);
    expect(first.commit.author).toBe("Ada");
    if (hasGit) {
      const porcelain = cli(f.repo.dir, "blame", "--porcelain", "HEAD", "--", "src/lib.txt")
        .split("\n")
        .map((l) => /^([0-9a-f]{40}) \d+ (\d+)/.exec(l))
        .filter((m): m is RegExpExecArray => m !== null);
      expect(porcelain.map((m) => m[1])).toEqual(perLine(blame));
    }
  });

  it("blames an old revision, takes collection paths, and refuses missing and binary files", async () => {
    const f = await blameFixture();
    const history = api(f.repo);
    expect(perLine(await history.blame(f.c2, "/src/main.txt"))).toEqual([f.c1, f.c2, f.c1, f.c1, f.c2]);
    expect((await history.blame("HEAD", "src/lib.txt")).lines).toBe(6);
    await expect(history.blame("HEAD", "/nope.txt")).rejects.toThrow();
    await expect(history.blame("HEAD", "/../etc/passwd")).rejects.toThrow();
    const bin = await snapshot(f.repo, [f.merge], { "logo.png": new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0, 0, 0]), "src/lib.txt": "x\n" }, "binary", 6000, "Ada");
    await ref(f.repo, "refs/heads/bin", bin);
    const b = await history.blame("bin", "/logo.png");
    expect(b).toMatchObject({ binary: true, lines: 0, hunks: [] });
  });

  it("handles an empty file and a last line without a newline", async () => {
    const repo = newRepo();
    await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
    const c1 = await snapshot(repo, [], { "a.txt": "", "b.txt": "one\ntwo" }, "one", 1000, "Ada");
    const c2 = await snapshot(repo, [c1], { "a.txt": "", "b.txt": "one\ntwo\nthree" }, "two", 2000, "Grace");
    await ref(repo, "refs/heads/main", c2);
    const history = api(repo);
    expect(await history.blame("HEAD", "/a.txt")).toMatchObject({ lines: 0, hunks: [] });
    expect(perLine(await history.blame("HEAD", "/b.txt"))).toEqual([c1, c2, c2]);
  });
});

describe("file history of any file", () => {
  it("follows renames of files outside the collection and keeps the leading slash", async () => {
    const f = await blameFixture();
    const log = await followFileLog(f.repo, "/src/lib.txt", 50);
    expect(log.map((c) => [c.commit.oid, c.change, c.path, c.from ?? null])).toEqual([
      [f.merge, "modified", "/src/lib.txt", null],
      [f.m1, "modified", "/src/lib.txt", null],
      [f.f1, "modified", "/src/lib.txt", null],
      [f.c3, "renamed", "/src/lib.txt", "/src/main.txt"],
      [f.c2, "modified", "/src/main.txt", null],
      [f.c1, "added", "/src/main.txt", null],
    ]);
    expect(await followFileLog(f.repo, "/README.md", 10)).toHaveLength(1);
    expect(await followFileLog(f.repo, "/nope.txt", 10)).toEqual([]);
    await expect(followFileLog(f.repo, "/../x", 10)).rejects.toThrow();
  });
});

// ---------------------------------------------------------------- search

interface SearchFixture {
  repo: Repo;
  id: Record<string, string>;
}

async function searchFixture(): Promise<SearchFixture> {
  const repo = newRepo();
  await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
  const readme = "# Billing\n";
  const readmeTok = "# Billing\nUse the SECRET_TOKEN header\n";
  const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0, 0]);
  const c1 = await snapshot(repo, [], { "api/users.yml": "url: /users\n", "README.md": readme }, "Add users endpoint", 1000, "Ada");
  const c2 = await snapshot(repo, [c1], { "api/users.yml": "url: /users?page=1\n", "README.md": readme }, "Fix pagination", 2000, "Grace");
  const c3 = await snapshot(repo, [c2], { "api/users.yml": "url: /users?page=1\n", "README.md": readme, "logo.png": png }, "Add logo", 3000, "Linus");
  const f1 = await snapshot(repo, [c2], { "api/users.yml": "url: /users?page=1\n", "README.md": readmeTok }, "Document tokens", 3500, "Ada");
  const c4 = await snapshot(repo, [c3], { "api/people.yml": "url: /users?page=1\n", "README.md": readme, "logo.png": png }, "Rename users", 4000, "Grace");
  const merge = await snapshot(repo, [c4, f1], { "api/people.yml": "url: /users?page=1\n", "README.md": readmeTok, "logo.png": png }, "Merge feature", 5000, "Linus");
  const c5 = await snapshot(repo, [merge], { "api/people.yml": "url: /users?page=1\n", "README.md": readme, "logo.png": png }, "Remove token docs", 6000, "Ada");
  await ref(repo, "refs/heads/main", c5);
  await ref(repo, "refs/heads/feature", f1);
  return { repo, id: { c1, c2, c3, f1, c4, merge, c5 } };
}

const oids = (page: { hits: { commit: { oid: string } }[] }) => page.hits.map((h) => h.commit.oid);

describe("searchHistory", () => {
  it("finds by message and author, case-insensitively, every field having to match", async () => {
    const { repo, id } = await searchFixture();
    const h = api(repo);
    const fix = await h.searchHistory(query({ message: "FIX" }));
    expect(oids(fix)).toEqual([id.c2]);
    expect(fix.hits[0].matched).toEqual(["message"]);
    expect(fix.more).toBe(false);
    expect(oids(await h.searchHistory(query({ author: "grace" })))).toEqual([id.c4, id.c2]);
    expect(oids(await h.searchHistory(query({ author: "ada@example" })))).toEqual([id.c5, id.f1, id.c1]);
    const both = await h.searchHistory(query({ message: "a", author: "linus" }));
    expect(oids(both)).toEqual([id.merge, id.c3]);
    expect(both.hits[0].matched).toEqual(["message", "author"]);
  });

  it("finds by path and content like git log, merges not listed, moved lines not counted", async () => {
    const { repo, id } = await searchFixture();
    const h = api(repo);
    const api_ = await h.searchHistory(query({ path: "/api" }));
    expect(oids(api_)).toEqual([id.c4, id.c2, id.c1]);
    expect(api_.hits[0].paths).toEqual(["/api/people.yml", "/api/users.yml"]);
    expect(api_.hits[0].matched).toEqual(["path"]);
    expect(oids(await h.searchHistory(query({ path: "/logo.png" })))).toEqual([id.c3]);
    expect(oids(await h.searchHistory(query({ path: "/api/people" })))).toEqual([id.c4]);

    const token = await h.searchHistory(query({ content: "secret_token" }));
    expect(oids(token)).toEqual([id.c5, id.f1]);
    expect(token.hits[0].paths).toEqual(["/README.md"]);
    expect(token.hits[0].matched).toEqual(["content"]);
    const both = await h.searchHistory(query({ content: "page=1", path: "/api" }));
    expect(oids(both)).toEqual([id.c2]);
    expect(both.hits[0].matched).toEqual(["path", "content"]);
    expect((await h.searchHistory(query({ content: "nothing here" }))).hits).toEqual([]);

    if (hasGit) {
      const log = (...args: string[]) => cli(repo.dir, "log", "--all", "--date-order", "--format=%H", "--no-merges", "--full-history", ...args).split("\n").filter(Boolean);
      expect(oids(api_)).toEqual(log("--", "api"));
      expect(oids(token)).toEqual(log("-i", "-S", "SECRET_TOKEN"));
    }
  });

  it("pages are stable and cover the history once; time and refs restrict", async () => {
    const { repo, id } = await searchFixture();
    const h = api(repo);
    const all = await h.searchHistory(query());
    expect(oids(all)).toEqual([id.c5, id.merge, id.c4, id.f1, id.c3, id.c2, id.c1]);
    expect(all.more).toBe(false);
    const joined: string[] = [];
    for (let skip = 0; ; ) {
      const page = await h.searchHistory(query({ skip, limit: 3 }));
      expect(page.hits.length).toBeLessThanOrEqual(3);
      skip += page.hits.length;
      joined.push(...oids(page));
      if (!page.more) break;
    }
    expect(joined).toEqual(oids(all));
    expect(all.hits[0].commit.refs).toEqual(["refs/heads/main"]);
    expect(all.hits[0].commit.parents).toEqual([id.merge]);
    expect(oids(await h.searchHistory(query({ since: 2000, until: 3500 })))).toEqual([id.f1, id.c3, id.c2]);
    expect(oids(await h.searchHistory(query({ refs: ["refs/heads/feature"] })))).toEqual([id.f1, id.c2, id.c1]);
    await expect(h.searchHistory(query({ refs: ["nope"] }))).rejects.toThrow();
    if (hasGit) expect(oids(all)).toEqual(cli(repo.dir, "log", "--all", "--date-order", "--format=%H").split("\n").filter(Boolean));
  });

  it("an empty repository has no hits", async () => {
    const repo = newRepo();
    await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
    const page = await api(repo).searchHistory(query());
    expect(page.hits).toEqual([]);
    expect(page.more).toBe(false);
  });
});

// ---------------------------------------------------------------- reflog

describe("reflog", () => {
  it("reads .git/logs: newest first, old and new ids, messages, a limit, short branch names", async () => {
    const repo = newRepo();
    await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
    const c1 = await snapshot(repo, [], { "a.txt": "one\n" }, "first", 1_700_000_000, "Ada");
    const c2 = await snapshot(repo, [c1], { "a.txt": "two\n" }, "second", 1_700_000_100, "Ada");
    const zero = "0".repeat(40);
    const sig = "Ada <ada@example.com> 1700000100 +0000";
    nodeFs.mkdirSync(join(repo.dir, ".git/logs/refs/heads"), { recursive: true });
    const lines = [`${zero} ${c1} Ada <ada@example.com> 1700000000 +0000\tcommit (initial): first`, `${c1} ${c2} ${sig}\tcommit: second`, `${c2} ${c1} ${sig}\treset: moving to HEAD~1`];
    nodeFs.writeFileSync(join(repo.dir, ".git/logs/HEAD"), `${lines.join("\n")}\n`);
    nodeFs.writeFileSync(join(repo.dir, ".git/logs/refs/heads/main"), `${lines.join("\n")}\n`);
    const h = api(repo);

    const log = await h.reflog("HEAD", 50);
    expect(log.map((e) => e.index)).toEqual([0, 1, 2]);
    expect(log[0]).toMatchObject({ name: "HEAD", old: c2, new: c1, message: "reset: moving to HEAD~1", time: 1_700_000_100 });
    expect(log[1]).toMatchObject({ old: c1, new: c2 });
    expect(log[1].commit?.summary).toBe("second");
    expect(log[2]).toMatchObject({ old: null, new: c1 });
    expect((await h.reflog("main", 50)).length).toBe(3);
    expect((await h.reflog("refs/heads/main", 2)).length).toBe(2);
    expect(await h.reflog("nobranch", 10)).toEqual([]);
    await expect(h.reflog("", 10)).rejects.toThrow();
    await expect(h.reflog("refs/../x", 10)).rejects.toThrow();
    // A commit the repository does not have (garbage collected) has no details.
    nodeFs.appendFileSync(join(repo.dir, ".git/logs/HEAD"), `${c1} ${"1".repeat(40)} ${sig}\tcheckout: elsewhere\n`);
    const gone = await h.reflog("HEAD", 1);
    expect(gone[0].commit).toBeUndefined();
  });
});
