// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A collection in a subfolder of its repository: the web Git runs at the repository's
// folder, the wave-12 paths stay relative to the collection (a leading `/`: the repository).

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { GitActionMethods } from "@requesttailor/ui";
import { afterEach, describe, expect, it } from "vitest";

import type { WebHost } from "../web-backend";
import { webGitActions } from "./actions";
import type { FsClient } from "./fs";
import * as repoOps from "./repo";
import type { Repo } from "./repo";
import { fileLog, show, treeAt } from "./revisions";
import { collectionPath, repoPath, scopedPath } from "./scope";

const me = { name: "Test User", email: "test@example.com" };
const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

const REQUEST = "method: GET\nurl: https://api.test/users\n";

/** A repository with the collection in `api/` and a file outside it, one commit. */
async function nested() {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-scope-"));
  dirs.push(dir);
  const write = (p: string, t: string) => {
    nodeFs.mkdirSync(join(dir, p, ".."), { recursive: true });
    nodeFs.writeFileSync(join(dir, p), t);
  };
  const files = { "api/opencollection.yml": "opencollection: 1.0.0\n", "api/users/list.yml": REQUEST, "notes.txt": "a\n" };
  for (const [p, t] of Object.entries(files)) write(p, t);
  const whole: Repo = { fs: nodeFs as unknown as FsClient, dir, cache: {} };
  await repoOps.init(whole);
  await repoOps.commit(whole, "first", Object.keys(files), me);
  const r: Repo = { ...whole, scope: "api" };
  const read = (p: string) => (nodeFs.existsSync(join(dir, p)) ? nodeFs.readFileSync(join(dir, p), "utf8") : null);
  return { r, write, read };
}

function actions(r: Repo): GitActionMethods {
  const host = {
    deps: {},
    session: () => ({ ws: { dirty: () => [] }, handle: {} }),
    openHandle: async () => ({}),
    companionCredentials: async () => null,
  } as unknown as WebHost;
  return webGitActions({
    host,
    repo: () => ({ ...r, cache: {} }),
    run: async (op) => {
      try {
        return await op();
      } catch (e) {
        throw repoOps.gitError(e);
      }
    },
    network: async () => {
      throw new Error("no network in this test");
    },
    identity: () => me,
  });
}

describe("paths between collection and repository", () => {
  const r = { scope: "api" } as Repo;
  it("maps both ways, a leading / meaning the repository", () => {
    expect(repoPath(r, "users/list.yml")).toBe("api/users/list.yml");
    expect(repoPath(r, "/notes.txt")).toBe("notes.txt");
    expect(scopedPath(r, "api/users/list.yml")).toBe("users/list.yml");
    expect(scopedPath(r, "apis/x.yml")).toBeNull();
    expect(collectionPath(r, "notes.txt")).toBe("/notes.txt");
    const root = {} as Repo;
    expect(repoPath(root, "a.yml")).toBe("a.yml");
    expect(collectionPath(root, "a.yml")).toBe("a.yml");
  });
});

describe("a collection in a subfolder of the repository", () => {
  it("shows, diffs and commits the collection's files by their collection paths", async () => {
    const { r, write } = await nested();
    write("api/users/list.yml", REQUEST.replace("GET", "POST"));
    write("notes.txt", "b\n");
    const s = await repoOps.status(r);
    expect(s.repo).toBe(true);
    expect(s.files).toEqual([{ path: "users/list.yml", state: "modified" }]);
    const [old, now] = await repoOps.diffSides(r, "users/list.yml");
    expect(new TextDecoder().decode(old!)).toContain("GET");
    expect(new TextDecoder().decode(now!)).toContain("POST");
    await repoOps.commit(r, "post", ["users/list.yml"], me);
    expect((await repoOps.status(r)).files).toEqual([]);
    // The file outside the collection is still changed, in the repository's status.
    expect((await repoOps.status({ ...r, scope: "" })).files).toEqual([{ path: "notes.txt", state: "modified" }]);
  });

  it("reads revisions relative to the collection", async () => {
    const { r, write } = await nested();
    write("api/users/list.yml", REQUEST.replace("GET", "PUT"));
    await repoOps.commit(r, "put", ["users/list.yml"], me);
    const tree = await treeAt(r, "HEAD");
    expect(Object.keys(tree.files).sort()).toEqual(["opencollection.yml", "users/list.yml"]);
    expect(await show(r, "HEAD~1", "users/list.yml")).toBe(REQUEST);
    expect(await show(r, "HEAD", "/notes.txt")).toBe("a\n");
    const log = await fileLog(r, "users/list.yml", 10);
    expect(log.map((c) => [c.path, c.change])).toEqual([
      ["users/list.yml", "modified"],
      ["users/list.yml", "added"],
    ]);
  });

  it("discards a collection path the panel lists", async () => {
    const { r, write, read } = await nested();
    write("api/users/list.yml", "changed: true\n");
    const api = actions(r);
    const plan = await api.plan({ action: "discard", paths: ["users/list.yml"] });
    expect(plan.blocked).toBeFalsy();
    expect(plan.files).toEqual(["/api/users/list.yml"]);
    await api.apply({ action: "discard", paths: ["users/list.yml"] });
    expect(read("api/users/list.yml")).toBe(REQUEST);
  });
});
