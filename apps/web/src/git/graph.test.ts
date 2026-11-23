// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand F: the web app's commit graph on isomorphic-git (Node's fs), the same cases
// as crates/rt-git/tests/graph.rs: all refs with kinds, upstream, ahead/behind and tag
// messages; date order with merges and two roots; stable pages; chosen refs; a detached HEAD;
// changes with renames (same content and similar), binary files, ranges and merges; the
// collection named for collection files; both sides of a file diff.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import git from "isomorphic-git";
import { afterEach, describe, expect, it } from "vitest";

import type { FsClient } from "./fs";
import * as graph from "./graph";
import type { Repo } from "./repo";

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

const enc = new TextEncoder();
const dec = new TextDecoder();
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52]);
const COLLECTION = "opencollection: 1.0.0\ninfo:\n  name: Billing\n";

function newRepo(): Repo {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-graph-"));
  dirs.push(dir);
  return { fs: nodeFs as unknown as FsClient, dir, cache: {} };
}

type Files = Record<string, string | Uint8Array>;

async function writeTree(repo: Repo, files: Files): Promise<string> {
  const here: { mode: string; path: string; oid: string; type: "blob" | "tree" }[] = [];
  const dirsBelow = new Map<string, Files>();
  for (const [p, content] of Object.entries(files)) {
    const [head, ...rest] = p.split("/");
    if (rest.length > 0) {
      const sub = dirsBelow.get(head) ?? {};
      sub[rest.join("/")] = content;
      dirsBelow.set(head, sub);
    } else {
      const blob = typeof content === "string" ? enc.encode(content) : content;
      here.push({ mode: "100644", path: head, oid: await git.writeBlob({ fs: repo.fs, dir: repo.dir, blob }), type: "blob" });
    }
  }
  for (const [name, sub] of dirsBelow) here.push({ mode: "040000", path: name, oid: await writeTree(repo, sub), type: "tree" });
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

interface Fixture {
  repo: Repo;
  c1: string;
  c2: string;
  f1: string;
  f2: string;
  merge: string;
  c4: string;
  docs: string;
  remoteOnly: string;
  localOnly: string;
}

/** The history of crates/rt-git/tests/graph.rs. */
async function fixture(): Promise<Fixture> {
  const repo = newRepo();
  await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
  const readme = "# Billing\n";
  const post = "method: POST\nurl: /v1/pay\nbody: x\n";
  const c1 = await snapshot(repo, [], { "README.md": readme, "api/opencollection.yml": COLLECTION, "api/get.yml": "url: /v1\n", "api/post.yml": post }, "Start the billing API", 1_000, "Ada");
  const c2 = await snapshot(repo, [c1], { "README.md": readme, "api/opencollection.yml": COLLECTION, "api/get.yml": "url: /v2\n", "api/post.yml": post }, "Version 2 of get", 2_000, "Ada");
  const f1 = await snapshot(repo, [c1], { "README.md": readme, "api/opencollection.yml": COLLECTION, "api/get.yml": "url: /v1\n", "api/post.yml": post, "logo.png": PNG }, "Add a logo", 1_500, "Grace");
  const postY = post.replace("body: x", "body: y");
  const f2 = await snapshot(repo, [f1], { "README.md": readme, "api/opencollection.yml": COLLECTION, "api/get.yml": "url: /v1\n", "api/post.yml": postY, "logo.png": PNG }, "Body y", 2_500, "Grace");
  const merged = { "README.md": readme, "api/opencollection.yml": COLLECTION, "api/get.yml": "url: /v2\n", "api/post.yml": postY, "logo.png": PNG };
  const merge = await snapshot(repo, [c2, f2], merged, "Merge feature", 3_000, "Ada");
  const latest = { "README.md": "# Billing\n\nPayments.\n", "api/opencollection.yml": COLLECTION, "api/get.yml": "url: /v2\n", "api/create.yml": postY, "logo.png": PNG };
  const c4 = await snapshot(repo, [merge], latest, "Rename post to create", 4_000, "Ada");
  const docs = await snapshot(repo, [], { "guide.md": "# Guide\n" }, "Docs", 500, "Linus");
  const remoteOnly = await snapshot(repo, [c4], { "README.md": "# Billing (remote)\n" }, "Remote edit", 5_000, "Linus");
  const localOnly = await snapshot(repo, [c4], latest, "Local edit", 6_000, "Ada");
  await ref(repo, "refs/heads/main", localOnly);
  await ref(repo, "refs/heads/feature", f2);
  await ref(repo, "refs/heads/docs", docs);
  await ref(repo, "refs/tags/v0", c1);
  await git.annotatedTag({
    fs: repo.fs,
    dir: repo.dir,
    ref: "v1",
    object: merge,
    message: "First release\n\nWith notes.\n",
    tagger: { name: "Ada", email: "ada@example.com", timestamp: 3_000, timezoneOffset: 0 },
  });
  await git.addRemote({ fs: repo.fs, dir: repo.dir, remote: "origin", url: "https://git.example.com/billing.git" });
  await ref(repo, "refs/remotes/origin/main", remoteOnly);
  await ref(repo, "refs/remotes/origin/feature", f2);
  await git.setConfig({ fs: repo.fs, dir: repo.dir, path: "branch.main.remote", value: "origin" });
  await git.setConfig({ fs: repo.fs, dir: repo.dir, path: "branch.main.merge", value: "refs/heads/main" });
  await git.checkout({ fs: repo.fs, dir: repo.dir, ref: "main" });
  nodeFs.writeFileSync(join(repo.dir, "README.md"), "# Billing\n\nPayments in EUR.\n");
  return { repo, c1, c2, f1, f2, merge, c4, docs, remoteOnly, localOnly };
}

const ids = (page: { commits: { oid: string }[] }) => page.commits.map((c) => c.oid);

describe("web commit graph", () => {
  it("lists all refs with kinds, upstream, ahead/behind and tag messages", async () => {
    const f = await fixture();
    const refs = await graph.listRefs(f.repo);
    expect(refs.map((r) => [r.full, r.kind])).toEqual([
      ["refs/heads/docs", "local"],
      ["refs/heads/feature", "local"],
      ["refs/heads/main", "local"],
      ["refs/remotes/origin/feature", "remote"],
      ["refs/remotes/origin/main", "remote"],
      ["refs/tags/v0", "tag"],
      ["refs/tags/v1", "tag"],
    ]);
    const main = refs.find((r) => r.name === "main")!;
    expect(main).toMatchObject({ current: true, target: f.localOnly, upstream: "origin/main", ahead: 1, behind: 1 });
    expect(refs.find((r) => r.name === "feature")).toMatchObject({ current: false, ahead: 0, behind: 0 });
    expect(refs.find((r) => r.name === "feature")!.upstream).toBeUndefined();
    expect(refs.find((r) => r.name === "v1")).toMatchObject({ target: f.merge, message: "First release" });
    expect(refs.find((r) => r.name === "v0")!.message).toBeUndefined();
  });

  it("orders all commits topologically, newest first", async () => {
    const f = await fixture();
    const page = await graph.graph(f.repo, { skip: 0, limit: 100 });
    expect(ids(page)).toEqual([f.localOnly, f.remoteOnly, f.c4, f.merge, f.f2, f.c2, f.f1, f.c1, f.docs]);
    expect(page.more).toBe(false);
    expect(page.head).toBe(f.localOnly);
    expect(page.detached).toBe(false);
    const merge = page.commits[3];
    expect(merge).toMatchObject({ summary: "Merge feature", parents: [f.c2, f.f2], refs: ["refs/tags/v1"], author: "Ada", email: "ada@example.com", time: 3_000 });
    expect(page.commits[4].refs).toEqual(["refs/heads/feature", "refs/remotes/origin/feature"]);
    expect(page.commits[8].parents).toEqual([]);
  });

  it("pages stably", async () => {
    const f = await fixture();
    const all = ids(await graph.graph(f.repo, { skip: 0, limit: 100 }));
    const paged: string[] = [];
    for (let skip = 0; ; ) {
      const page = await graph.graph(f.repo, { skip, limit: 4 });
      expect(page.refs).toHaveLength(7);
      paged.push(...ids(page));
      skip += page.commits.length;
      if (!page.more) break;
    }
    expect(paged).toEqual(all);
    const past = await graph.graph(f.repo, { skip: 50, limit: 4 });
    expect(past.commits).toEqual([]);
    expect(past.more).toBe(false);
  });

  it("starts from chosen refs", async () => {
    const f = await fixture();
    expect(ids(await graph.graph(f.repo, { limit: 100, refs: ["refs/heads/feature"] }))).toEqual([f.f2, f.f1, f.c1]);
    expect(ids(await graph.graph(f.repo, { limit: 100, refs: ["HEAD"] }))[0]).toBe(f.localOnly);
    await expect(graph.graph(f.repo, { limit: 1, refs: ["refs/heads/nope"] })).rejects.toThrow(/Unknown revision/);
  });

  it("shows a detached HEAD as a ref of its own", async () => {
    const f = await fixture();
    nodeFs.writeFileSync(join(f.repo.dir, ".git/HEAD"), `${f.c2}\n`);
    const refs = await graph.listRefs(f.repo);
    const head = refs.find((r) => r.kind === "head")!;
    expect(head).toMatchObject({ full: "HEAD", name: "HEAD", current: true, target: f.c2 });
    expect(refs[0]).toBe(head);
    expect(refs.filter((r) => r.current)).toHaveLength(1);
    const page = await graph.graph(f.repo, { limit: 100 });
    expect(page.detached).toBe(true);
    expect(page.head).toBe(f.c2);
    expect(page.commits.find((c) => c.oid === f.c2)!.refs).toEqual(["HEAD"]);
  });

  it("has an empty graph without commits", async () => {
    const repo = newRepo();
    await git.init({ fs: repo.fs, dir: repo.dir, defaultBranch: "main" });
    const page = await graph.graph(repo, { limit: 10 });
    expect(page).toEqual({ commits: [], refs: [], detached: false, more: false });
  });

  it("lists what a commit changed: renames, binary files, ranges, merges", async () => {
    const f = await fixture();
    const c4 = await graph.changes(f.repo, null, f.c4, null);
    expect(c4.map((c) => [c.path, c.change, c.from])).toEqual([
      ["/README.md", "modified", undefined],
      ["/api/create.yml", "renamed", "/api/post.yml"],
    ]);
    const f1 = await graph.changes(f.repo, null, f.f1, null);
    expect(f1).toEqual([{ path: "/logo.png", change: "added", binary: true }]);
    expect((await graph.changes(f.repo, null, f.docs, null)).map((c) => c.change)).toEqual(["added"]);
    expect((await graph.changes(f.repo, f.c1, f.c4, null)).map((c) => c.path)).toEqual(["/README.md", "/api/create.yml", "/api/get.yml", "/logo.png"]);
    expect((await graph.changes(f.repo, null, f.merge, null)).map((c) => c.path)).toEqual(["/api/post.yml", "/logo.png"]);
    const edited = await graph.changes(f.repo, f.c2, f.c4, null);
    expect(edited.find((c) => c.path === "/api/create.yml")).toMatchObject({ change: "renamed", from: "/api/post.yml" });
    expect(edited.some((c) => c.path === "/api/post.yml")).toBe(false);
  });

  it("names the active collection for its files (the root is the collection folder)", async () => {
    const f = await fixture();
    // In the browser the collection is the repository root; api/ has its own
    // opencollection.yml, so it is a nested collection and its files are not the root's.
    const files = await graph.changes(f.repo, f.c1, f.c4, "root");
    expect(files.map((c) => [c.path, c.collection])).toEqual([
      ["/README.md", undefined],
      ["/api/create.yml", undefined],
      ["/api/get.yml", undefined],
      ["/logo.png", undefined],
    ]);
    const flat = newRepo();
    await git.init({ fs: flat.fs, dir: flat.dir, defaultBranch: "main" });
    const a = await snapshot(flat, [], { "opencollection.yml": COLLECTION, "get.yml": "url: /v1\n", "notes.md": "x\n" }, "a", 1, "Ada");
    const b = await snapshot(flat, [a], { "opencollection.yml": COLLECTION, "get.yml": "url: /v2\n", "notes.md": "y\n" }, "b", 2, "Ada");
    expect((await graph.changes(flat, null, b, "root")).map((c) => [c.path, c.collection])).toEqual([
      ["/get.yml", "root"],
      ["/notes.md", undefined],
    ]);
  });

  it("reads both sides of a file diff", async () => {
    const f = await fixture();
    const text = (b: Uint8Array | null) => (b ? dec.decode(b) : null);
    const get = await graph.fileSides(f.repo, f.c1, f.c2, "/api/get.yml");
    expect([get.path, text(get.old), text(get.new)]).toEqual(["/api/get.yml", "url: /v1\n", "url: /v2\n"]);
    const parent = await graph.fileSides(f.repo, null, f.c2, "/api/get.yml");
    expect(text(parent.old)).toBe("url: /v1\n");
    // A renamed file diffs against its old path.
    const renamed = await graph.fileSides(f.repo, f.c2, f.c4, "/api/create.yml");
    expect(text(renamed.old)).toContain("body: x");
    expect(text(renamed.new)).toContain("body: y");
    const logo = await graph.fileSides(f.repo, null, f.f1, "/logo.png");
    expect(logo.old).toBeNull();
    expect(graph.isBinary(logo.new!)).toBe(true);
    // The work tree, against HEAD or a revision.
    const work = await graph.fileSides(f.repo, null, null, "/README.md");
    expect([text(work.old), text(work.new)]).toEqual(["# Billing\n\nPayments.\n", "# Billing\n\nPayments in EUR.\n"]);
    expect(text((await graph.fileSides(f.repo, f.c1, null, "/README.md")).old)).toBe("# Billing\n");
    await expect(graph.fileSides(f.repo, null, null, "/../x")).rejects.toThrow(/Invalid path/);
    await expect(graph.fileSides(f.repo, null, null, "/.git/config")).rejects.toThrow(/Invalid path/);
  });

  it("measures similarity like the native shell", () => {
    expect(graph.similarity("a\nb\nc\n", "a\nb\nd\n")).toBeCloseTo(2 / 3);
    expect(graph.similarity("", "")).toBe(1);
    expect(graph.isBinary(enc.encode("Grüße"))).toBe(false);
    expect(graph.isBinary(new Uint8Array([0xff, 0xfe]))).toBe(true);
  });
});
