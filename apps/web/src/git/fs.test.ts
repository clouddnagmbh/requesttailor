// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import git from "isomorphic-git";
import { describe, expect, it } from "vitest";

import { fill, memoryFolder } from "./fake-handle.test-util";
import { directoryFs, splitPath } from "./fs";

const code = (p: Promise<unknown>) => p.then(() => "ok", (e: { code?: string }) => e.code ?? String(e));

describe("directoryFs", () => {
  it("reads and writes files as bytes or text", async () => {
    const root = memoryFolder();
    const { promises: fs } = directoryFs(root);
    await fs.mkdir("/a");
    await fs.writeFile("/a/x.txt", "hello");
    await fs.writeFile("a/y.bin", new Uint8Array([0, 1, 2]));
    expect(await fs.readFile("/a/x.txt", "utf8")).toBe("hello");
    expect(await fs.readFile("/a/x.txt", { encoding: "utf8" })).toBe("hello");
    expect(await fs.readFile("/a/y.bin")).toEqual(new Uint8Array([0, 1, 2]));
    expect((await fs.readdir("/a")).sort()).toEqual(["x.txt", "y.bin"]);
    expect((await fs.readdir("/")).sort()).toEqual(["a"]);
  });

  it("reports Node error codes isomorphic-git relies on", async () => {
    const root = memoryFolder();
    await fill(root, { "dir/file.txt": "x" });
    const { promises: fs } = directoryFs(root);
    expect(await code(fs.readFile("/missing"))).toBe("ENOENT");
    expect(await code(fs.readFile("/dir"))).toBe("EISDIR");
    expect(await code(fs.readFile("/dir/file.txt/deeper"))).toBe("ENOTDIR");
    expect(await code(fs.readdir("/dir/file.txt"))).toBe("ENOTDIR");
    expect(await code(fs.readdir("/nope"))).toBe("ENOENT");
    expect(await code(fs.writeFile("/no/such/dir.txt", "x"))).toBe("ENOENT");
    expect(await code(fs.mkdir("/dir"))).toBe("EEXIST");
    expect(await code(fs.mkdir("/dir/file.txt"))).toBe("EEXIST");
    expect(await code(fs.mkdir("/a/b"))).toBe("ENOENT");
    expect(await code(fs.rmdir("/dir"))).toBe("ENOTEMPTY");
    expect(await code(fs.rmdir("/dir/file.txt"))).toBe("ENOTDIR");
    expect(await code(fs.unlink("/dir"))).toBe("EISDIR");
    expect(await code(fs.unlink("/gone"))).toBe("ENOENT");
    expect(await code(fs.stat("/gone"))).toBe("ENOENT");
    expect(await code(fs.readlink("/dir"))).toBe("ENOTSUP");
    // isomorphic-git probes the API with `readFile()`: it must reject, not throw.
    expect(await code(fs.readFile(undefined as unknown as string))).toBe("EINVAL");
  });

  it("stats files and folders", async () => {
    const root = memoryFolder();
    await fill(root, { "d/f.txt": "12345" });
    const { promises: fs } = directoryFs(root);
    const f = await fs.stat("/d/f.txt");
    expect(f.isFile()).toBe(true);
    expect(f.isDirectory()).toBe(false);
    expect(f.size).toBe(5);
    expect(f.mode).toBe(0o100644);
    expect(f.mtimeMs).toBeGreaterThan(0);
    const d = await fs.lstat("/d");
    expect(d.isDirectory()).toBe(true);
    expect((await fs.stat("/")).isDirectory()).toBe(true);
  });

  it("removes files and folders, recursively with rm", async () => {
    const root = memoryFolder();
    await fill(root, { "d/e/f.txt": "x", "g.txt": "y" });
    const { promises: fs } = directoryFs(root);
    await fs.unlink("/g.txt");
    await fs.unlink("/d/e/f.txt");
    await fs.rmdir("/d/e");
    expect(await fs.readdir("/d")).toEqual([]);
    await fill(root, { "d/e/f.txt": "x" });
    await fs.rm("/d", { recursive: true });
    expect(await fs.readdir("/")).toEqual([]);
  });

  it("never leaves the folder", () => {
    expect(splitPath("/a/./b/")).toEqual(["a", "b"]);
    expect(() => splitPath("/a/../../etc")).toThrow(/leaves the folder/);
  });

  it("carries a whole isomorphic-git round trip", async () => {
    const root = memoryFolder();
    await fill(root, { "opencollection.yml": "name: x\n", "r/one.yml": "a: 1\n" });
    const fs = directoryFs(root);
    const author = { name: "T", email: "t@example.com" };
    await git.init({ fs, dir: "/", defaultBranch: "main" });
    await git.add({ fs, dir: "/", filepath: "." });
    const oid = await git.commit({ fs, dir: "/", message: "first", author });
    expect(oid).toMatch(/^[0-9a-f]{40}$/);
    expect(await git.statusMatrix({ fs, dir: "/" })).toEqual([
      ["opencollection.yml", 1, 1, 1],
      ["r/one.yml", 1, 1, 1],
    ]);
    await fs.promises.writeFile("/r/one.yml", "a: 2\n");
    expect(await git.statusMatrix({ fs, dir: "/", filepaths: ["r"] })).toEqual([["r/one.yml", 1, 2, 1]]);
    await git.branch({ fs, dir: "/", ref: "next", checkout: false });
    expect(await git.listBranches({ fs, dir: "/" })).toEqual(["main", "next"]);
  });
});
