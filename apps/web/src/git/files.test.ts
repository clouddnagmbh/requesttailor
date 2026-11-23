// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B: the web app's working copy, file tree and file contents on isomorphic-git
// (Node's fs), the same cases as crates/rt-git/tests/working_copy.rs: changed, new and deleted
// files of the whole tree, staged and unstaged, renames, ignored files, detached HEAD, a merge
// in progress, the file tree at a revision and in the working tree, text, images, binary
// files and Git LFS pointers.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import git from "isomorphic-git";
import { afterEach, describe, expect, it } from "vitest";

import { BLOB_TEXT_LIMIT, base64, describeBlob, mediaType } from "./blob-info";
import type { FsClient } from "./fs";
import * as files from "./files";
import type { Repo } from "./repo";

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

const author = { name: "Ada", email: "ada@example.com" };
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52]);

function put(repo: Repo, rel: string, content: string | Uint8Array) {
  const file = join(repo.dir, rel);
  nodeFs.mkdirSync(dirname(file), { recursive: true });
  nodeFs.writeFileSync(file, content);
}

async function commitAll(repo: Repo, message = "commit") {
  await git.add({ fs: nodeFs, dir: repo.dir, filepath: ".", cache: repo.cache });
  return git.commit({ fs: nodeFs, dir: repo.dir, message, author, cache: repo.cache });
}

async function fixture(initial: Record<string, string | Uint8Array>): Promise<Repo> {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-files-"));
  dirs.push(dir);
  const repo: Repo = { fs: nodeFs as unknown as FsClient, dir, cache: {} };
  await git.init({ fs: nodeFs, dir, defaultBranch: "main" });
  for (const [p, c] of Object.entries(initial)) put(repo, p, c);
  await commitAll(repo, "first");
  return repo;
}

const row = (wc: Awaited<ReturnType<typeof files.workingCopy>>, path: string) => {
  const r = wc.files.find((f) => f.path === path);
  if (!r) throw new Error(`${path} not in ${JSON.stringify(wc.files)}`);
  return r;
};
const paths = (wc: Awaited<ReturnType<typeof files.workingCopy>>) => wc.files.map((f) => f.path);

describe("workingCopy", () => {
  it("lists changed, new and deleted files of the whole tree", async () => {
    const repo = await fixture({ "README.md": "hello\n", "api/a.yml": "a: 1\n", "api/b.yml": "b: 1\n", "src/deep/x.rs": "x\n" });
    expect((await files.workingCopy(repo, null, "r")).files).toEqual([]);
    put(repo, "api/a.yml", "a: 2\n");
    nodeFs.rmSync(join(repo.dir, "api/b.yml"));
    put(repo, "docs/guide.md", "# guide\n");
    const wc = await files.workingCopy(repo, null, "r");
    expect(paths(wc)).toEqual(["/api/a.yml", "/api/b.yml", "/docs/guide.md"]);
    expect(row(wc, "/api/a.yml")).toMatchObject({ state: "modified", unstaged: true });
    expect(row(wc, "/api/b.yml").state).toBe("deleted");
    expect(row(wc, "/docs/guide.md").state).toBe("untracked");
    expect(wc).toMatchObject({ branch: "main", detached: false, truncated: false, repository: "r" });
    expect(wc.head).toMatch(/^[0-9a-f]{40}$/);
  });

  it("tells staged and unstaged apart", async () => {
    const repo = await fixture({ "a.txt": "1\n", "b.txt": "1\n", "c.txt": "1\n" });
    put(repo, "a.txt", "2\n");
    await git.add({ fs: nodeFs, dir: repo.dir, filepath: "a.txt" });
    put(repo, "b.txt", "2\n");
    await git.add({ fs: nodeFs, dir: repo.dir, filepath: "b.txt" });
    put(repo, "b.txt", "3\n");
    put(repo, "new.txt", "n\n");
    await git.add({ fs: nodeFs, dir: repo.dir, filepath: "new.txt" });
    await git.remove({ fs: nodeFs, dir: repo.dir, filepath: "c.txt" });
    nodeFs.rmSync(join(repo.dir, "c.txt"));
    const wc = await files.workingCopy(repo, null, "r");
    expect(row(wc, "/a.txt")).toMatchObject({ state: "modified", staged: "modified", unstaged: false });
    expect(row(wc, "/b.txt")).toMatchObject({ state: "modified", staged: "modified", unstaged: true });
    expect(row(wc, "/new.txt")).toMatchObject({ state: "untracked", staged: "added", unstaged: false });
    expect(row(wc, "/c.txt")).toMatchObject({ state: "deleted", staged: "deleted" });
  });

  it("names the old path of a rename and keeps both rows", async () => {
    const repo = await fixture({ "old/name.yml": "same content\n", "keep.txt": "k\n" });
    nodeFs.mkdirSync(join(repo.dir, "new"));
    nodeFs.renameSync(join(repo.dir, "old/name.yml"), join(repo.dir, "new/name.yml"));
    const wc = await files.workingCopy(repo, null, "r");
    expect(row(wc, "/old/name.yml").state).toBe("deleted");
    expect(row(wc, "/new/name.yml")).toMatchObject({ state: "untracked", from: "/old/name.yml" });
  });

  it("never lists ignored files, unless tracked", async () => {
    const repo = await fixture({ ".gitignore": "target/\n*.log\n", "src/a.rs": "a\n" });
    put(repo, "target/debug/big.bin", "x");
    put(repo, "build.log", "x");
    put(repo, "src/b.rs", "b\n");
    expect(paths(await files.workingCopy(repo, null, "r"))).toEqual(["/src/b.rs"]);
  });

  it("knows a detached HEAD", async () => {
    const repo = await fixture({ "a.txt": "a\n" });
    const oid = await git.resolveRef({ fs: nodeFs, dir: repo.dir, ref: "HEAD" });
    await git.checkout({ fs: nodeFs, dir: repo.dir, ref: oid, force: true });
    const wc = await files.workingCopy(repo, null, "r");
    expect(wc.detached).toBe(true);
    expect(wc.branch).toBeNull();
    expect(wc.head).toBe(oid);
  });

  it("names the active collection on its files, not on others", async () => {
    const repo = await fixture({
      "billing/opencollection.yml": "opencollection: 1.0.0\n",
      "billing/get.yml": "a: 1\n",
      "billing/v2/opencollection.yml": "opencollection: 1.0.0\n",
      "billing/v2/get.yml": "a: 1\n",
      "README.md": "r\n",
    });
    for (const p of ["billing/get.yml", "billing/v2/get.yml", "README.md"]) put(repo, p, "changed\n");
    const wc = await files.workingCopy({ ...repo, scope: "billing" }, "billing-id", "r");
    expect(row(wc, "/billing/get.yml").collection).toBe("billing-id");
    expect(row(wc, "/billing/v2/get.yml").collection).toBeUndefined();
    expect(row(wc, "/README.md").collection).toBeUndefined();
  });

  it("says no repository outside Git", async () => {
    const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-files-"));
    dirs.push(dir);
    await expect(files.workingCopy({ fs: nodeFs as unknown as FsClient, dir, cache: {} }, null, "r")).rejects.toMatchObject({ kind: "git" });
  });
});

describe("fileTree", () => {
  it("lists a folder at a revision and in the working tree", async () => {
    const repo = await fixture({ ".gitignore": "*.log\nout/\n", "src/a.rs": "a\n", "src/b.rs": "b\n", "src/sub/c.rs": "c\n", "top.md": "top\n" });
    put(repo, "src/a.rs", "a changed\n");
    nodeFs.rmSync(join(repo.dir, "src/b.rs"));
    put(repo, "src/new.rs", "n\n");
    put(repo, "src/debug.log", "x");
    put(repo, "out/o.bin", "x");

    const atHead = await files.fileTree(repo, "HEAD", "/src");
    expect(atHead.map((e) => [e.name, e.kind])).toEqual([
      ["sub", "dir"],
      ["a.rs", "file"],
      ["b.rs", "file"],
    ]);
    expect(atHead[1]).toMatchObject({ path: "/src/a.rs", size: 2, ignored: false });
    expect(atHead.every((e) => e.state === undefined)).toBe(true);

    const now = await files.fileTree(repo, null, "src");
    const get = (n: string) => now.find((e) => e.name === n)!;
    expect(get("a.rs").state).toBe("modified");
    expect(get("b.rs").state).toBe("deleted");
    expect(get("new.rs").state).toBe("untracked");
    expect(get("debug.log")).toMatchObject({ ignored: true });
    expect(get("debug.log").state).toBeUndefined();
    expect(get("sub").kind).toBe("dir");

    const root = await files.fileTree(repo, null, "/");
    expect(root.find((e) => e.name === "out")?.ignored).toBe(true);
    expect(root.some((e) => e.name === ".git")).toBe(false);
    expect(root[0]!.kind).toBe("dir");
    expect(await files.fileTree(repo, "HEAD", "/nope")).toEqual([]);
    await expect(files.fileTree(repo, null, "../etc")).rejects.toMatchObject({ kind: "git" });
  });
});

describe("blob", () => {
  it("is text, an image or metadata, by revision and in the working tree", async () => {
    const repo = await fixture({
      "notes.md": "# Notes\nline\n",
      "img/logo.png": PNG,
      "data/blob.bin": new Uint8Array([1, 0, 2, 0, 255]),
      "bad.txt": new Uint8Array([0xff, 0xfe, 0x61]),
      "model.glb": "version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 12345\n",
      "icon.svg": '<svg xmlns="http://www.w3.org/2000/svg"/>\n',
    });
    for (const rev of [null, "HEAD"]) {
      const text = await files.blob(repo, rev, "/notes.md");
      expect(text).toMatchObject({ text: "# Notes\nline\n", binary: false, truncated: false, lfs_pointer: false, size: 13 });
      expect(text.oid).toBe((await git.hashBlob({ object: new TextEncoder().encode("# Notes\nline\n") })).oid);
      expect(text.rev !== undefined).toBe(rev !== null);
      const png = await files.blob(repo, rev, "/img/logo.png");
      expect(png).toMatchObject({ media_type: "image/png", binary: true });
      expect(png.text).toBeUndefined();
      expect(png.base64).toBe("iVBORw0KGgoAAAANSUhEUg==");
      const bin = await files.blob(repo, rev, "/data/blob.bin");
      expect(bin).toMatchObject({ binary: true, size: 5 });
      expect(bin.text === undefined && bin.base64 === undefined).toBe(true);
      expect((await files.blob(repo, rev, "/bad.txt")).binary).toBe(true);
      const lfs = await files.blob(repo, rev, "/model.glb");
      expect(lfs.lfs_pointer).toBe(true);
      expect(lfs.text).toContain("size 12345");
      const svg = await files.blob(repo, rev, "/icon.svg");
      expect(svg.binary).toBe(false);
      expect(svg.text && svg.base64).toBeTruthy();
    }
    await expect(files.blob(repo, null, "/missing.txt")).rejects.toMatchObject({ kind: "git" });
    await expect(files.blob(repo, "HEAD", "/missing.txt")).rejects.toMatchObject({ kind: "git" });
    await expect(files.blob(repo, null, "/../x")).rejects.toMatchObject({ kind: "git" });
  });

  it("matches the object id of git hash-object for working-tree files", async () => {
    const repo = await fixture({ "a.txt": "old\n" });
    put(repo, "a.txt", "new\n");
    const working = await files.blob(repo, null, "/a.txt");
    const { oid } = await git.hashBlob({ object: new TextEncoder().encode("new\n") });
    expect(working.oid).toBe(oid);
    expect((await files.blob(repo, "HEAD", "/a.txt")).text).toBe("old\n");
  });
});

describe("blob-info", () => {
  it("knows media types, base64 and the size limit", () => {
    expect(mediaType("/img/Logo.PNG")).toBe("image/png");
    expect(mediaType("/Makefile")).toBeUndefined();
    expect(mediaType("/dir.d/file")).toBeUndefined();
    expect(base64(new TextEncoder().encode("foobar"))).toBe("Zm9vYmFy");
    expect(base64(new TextEncoder().encode("fooba"))).toBe("Zm9vYmE=");
    const big = describeBlob("/big.txt", undefined, new Uint8Array(BLOB_TEXT_LIMIT + 1).fill(0x78), undefined);
    expect(big).toMatchObject({ truncated: true, binary: false });
    expect(big.text).toBeUndefined();
  });
});
