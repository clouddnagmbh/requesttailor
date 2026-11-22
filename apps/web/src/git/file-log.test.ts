// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `fileLog` follows renames and moves: the same history and the same expected log as
// crates/rt-git/tests/file_log.rs.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { similarity } from "./file-log";
import type { FsClient } from "./fs";
import * as repoOps from "./repo";
import type { Repo } from "./repo";
import * as rev from "./revisions";

const me = { name: "Test User", email: "test@example.com" };
const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

function newRepo() {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-flog-"));
  dirs.push(dir);
  const repo: Repo = { fs: nodeFs as unknown as FsClient, dir, cache: {} };
  return {
    repo,
    write(p: string, t: string) {
      nodeFs.mkdirSync(join(dir, p, ".."), { recursive: true });
      nodeFs.writeFileSync(join(dir, p), t);
    },
    read: (p: string) => nodeFs.readFileSync(join(dir, p), "utf8"),
    rm: (p: string) => nodeFs.rmSync(join(dir, p)),
  };
}

const request = (url: string, extra = "") =>
  `info:\n  name: Users\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: ${url}\n  auth: inherit\n${extra}`;

describe("fileLog", () => {
  it("follows renames and moves", async () => {
    const r = newRepo();
    await repoOps.init(r.repo);
    const commit = async (msg: string, files: string[]) => (await repoOps.commit(r.repo, msg, files, me)).oid;
    const move = (from: string, to: string) => {
      const t = r.read(from);
      r.rm(from);
      r.write(to, t);
    };
    r.write("opencollection.yml", "opencollection: 1.0.0\n");
    r.write("other.yml", request("{{base}}/other"));
    r.write("users/get.yml", request("{{base}}/users"));
    const c1 = await commit("add", ["opencollection.yml", "other.yml", "users/get.yml"]);
    r.write("users/get.yml", request("{{base}}/users?x=1"));
    const c2 = await commit("edit", ["users/get.yml"]);
    move("users/get.yml", "users/list.yml");
    const c3 = await commit("rename", ["users/get.yml", "users/list.yml"]);
    r.write("users/list.yml", request("{{base}}/users?x=2"));
    const c4 = await commit("edit again", ["users/list.yml"]);
    move("users/list.yml", "api/list.yml");
    r.write("api/list.yml", request("{{base}}/api/users?x=2"));
    const c5 = await commit("move", ["users/list.yml", "api/list.yml"]);
    r.write("api/list.yml", request("{{base}}/api/users?x=3", "  timeout: 5\n"));
    const c6 = await commit("edit last", ["api/list.yml"]);

    const log = await rev.fileLog(r.repo, "api/list.yml", 50);
    expect(log.map((c) => [c.commit.oid, c.change, c.path, c.from ?? null])).toEqual([
      [c6, "modified", "api/list.yml", null],
      [c5, "renamed", "api/list.yml", "users/list.yml"],
      [c4, "modified", "users/list.yml", null],
      [c3, "renamed", "users/list.yml", "users/get.yml"],
      [c2, "modified", "users/get.yml", null],
      [c1, "added", "users/get.yml", null],
    ]);
    expect(await rev.fileLog(r.repo, "api/list.yml", 3)).toHaveLength(3);
    expect((await rev.fileLog(r.repo, "other.yml", 50)).map((c) => c.change)).toEqual(["added"]);
    expect(await rev.fileLog(r.repo, "nope.yml", 10)).toEqual([]);
  });

  it("does not take an unrelated new file for a rename", async () => {
    const r = newRepo();
    await repoOps.init(r.repo);
    r.write("a.yml", "one\ntwo\nthree\nfour\n");
    await repoOps.commit(r.repo, "a", ["a.yml"], me);
    r.rm("a.yml");
    r.write("b.yml", "completely\ndifferent\ntext\nhere\n");
    const c2 = await repoOps.commit(r.repo, "swap", ["a.yml", "b.yml"], me);
    const log = await rev.fileLog(r.repo, "b.yml", 10);
    expect(log.map((c) => [c.commit.oid, c.change])).toEqual([[c2.oid, "added"]]);
  });

  it("measures similarity like the desktop", () => {
    expect(similarity("a\nb\n", "a\nb\n")).toBe(1);
    expect(similarity("a\nb\nc\nd\n", "a\nb\nx\ny\n")).toBe(0.5);
    expect(similarity("", "")).toBe(1);
  });
});
