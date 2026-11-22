// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: the web app's revisions on Node's fs, the same cases as
// crates/rt-git/tests/revisions.rs.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import type { FsClient } from "./fs";
import * as repoOps from "./repo";
import type { Repo } from "./repo";
import * as rev from "./revisions";

const me = { name: "Test User", email: "test@example.com" };
const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

function newRepo(): Repo & { write(p: string, t: string): void } {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-rev-"));
  dirs.push(dir);
  return {
    fs: nodeFs as unknown as FsClient,
    dir,
    cache: {},
    write: (p, t) => {
      nodeFs.mkdirSync(join(dir, p, ".."), { recursive: true });
      nodeFs.writeFileSync(join(dir, p), t);
    },
  };
}

describe("revisions", () => {
  it("tree paths are the files the shells read from disk", () => {
    expect(rev.treePath("opencollection.yml")).toBe(true);
    expect(rev.treePath("users/get.yml")).toBe(true);
    expect(rev.treePath("openapi.yaml")).toBe(false);
    expect(rev.treePath(".github/workflows/ci.yml")).toBe(false);
    expect(rev.treePath("node_modules/x/a.yml")).toBe(false);
    expect(rev.treePath(".hidden.yml")).toBe(true);
    const files: Record<string, string> = { "opencollection.yml": "", "a.yml": "", "sub/opencollection.yml": "", "sub/b.yml": "", "subway/c.yml": "" };
    rev.dropNested(files);
    expect(Object.keys(files).sort()).toEqual(["a.yml", "opencollection.yml", "subway/c.yml"]);
  });

  it("reads commits without a checkout", async () => {
    const r = newRepo();
    await repoOps.init(r);
    r.write("opencollection.yml", "opencollection: 1.0.0\n");
    r.write("get.yml", "url: /v1\n");
    r.write("notes.txt", "not a collection file\n");
    r.write("nested/opencollection.yml", "opencollection: 1.0.0\n");
    r.write("nested/n.yml", "skipped\n");
    const first = await repoOps.commit(r, "first", ["opencollection.yml", "get.yml", "notes.txt", "nested/opencollection.yml", "nested/n.yml"], me);
    r.write("get.yml", "url: /v2\n");
    const second = await repoOps.commit(r, "second", ["get.yml"], me);
    r.write("post.yml", "method: POST\n");
    const third = await repoOps.commit(r, "third", ["post.yml"], me);

    expect(await rev.revParse(r, "HEAD")).toBe(third.oid);
    expect(await rev.revParse(r, "HEAD~2")).toBe(first.oid);
    expect(await rev.revParse(r, "HEAD^")).toBe(second.oid);
    expect(await rev.revParse(r, "main~1")).toBe(second.oid);
    expect(await rev.revParse(r, third.oid.slice(0, 10))).toBe(third.oid);
    await expect(rev.revParse(r, "no-such-branch")).rejects.toThrow(/Unknown revision/);

    const tree = await rev.treeAt(r, first.oid);
    expect(tree.oid).toBe(first.oid);
    expect(Object.keys(tree.files).sort()).toEqual(["get.yml", "opencollection.yml"]);
    expect(tree.files["get.yml"]).toBe("url: /v1\n");
    expect(Object.keys((await rev.treeAt(r, "HEAD")).files)).toHaveLength(3);

    expect(await rev.show(r, "HEAD~1", "get.yml")).toBe("url: /v2\n");
    expect(await rev.show(r, first.oid, "post.yml")).toBeNull();
    expect(await rev.show(r, "HEAD", "/notes.txt")).toBe("not a collection file\n");
    await expect(rev.show(r, "HEAD", "../x.yml")).rejects.toThrow(/Invalid path/);

    expect(await rev.mergeBase(r, third.oid, first.oid)).toBe(first.oid);
    expect((await rev.commits(r, first.oid, "HEAD", 10)).map((c) => c.oid)).toEqual([third.oid, second.oid]);
    expect(await rev.commits(r, null, "HEAD", 2)).toHaveLength(2);

    const log = await rev.fileLog(r, "get.yml", 10);
    expect(log.map((e) => [e.commit.oid, e.change])).toEqual([
      [second.oid, "modified"],
      [first.oid, "added"],
    ]);
    expect(await rev.fileLog(r, "get.yml", 1)).toHaveLength(1);
  });
});
