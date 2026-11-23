// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D: the mock follows the rules of the new actions and building blocks (the same
// cases as crates/rt-git/tests/{amend,abort,restore,rewrite_branch,plumbing}.rs).

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import type { MockGit } from "./mock-git";
import type { GitAction } from "./types";

async function repo() {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  git.scenario("clean");
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  return { b, git };
}

async function edit(b: MockBackend, path: string, url: string) {
  const spec = await b.getRequest(path);
  await b.updateRequest(path, { ...spec, url });
  await b.save();
}

const reason = async (git: MockGit, action: GitAction) => (await git.plan(action)).blocked?.reason;

describe("amend", () => {
  it("replaces the last commit, keeps its parents and author, and undo brings it back", async () => {
    const { b, git } = await repo();
    await edit(b, "users/list-users.yml", "https://example.test/local");
    await git.commit("local", ["users/list-users.yml"]);
    const old = (await git.log(1))[0];
    await edit(b, "health.yml", "https://example.test/amended");
    const p = await git.plan({ action: "amend", message: "reworded", paths: ["/health.yml"] });
    expect(p).toMatchObject({ backup: true, undoable: true, files: ["/health.yml"] });
    const res = await git.apply(p.action);
    const now = (await git.log(1))[0];
    expect(now.summary).toBe("reworded");
    expect(now.oid).not.toBe(old.oid);
    expect(now.author).toBe(old.author);
    expect((await git.status()).files).toEqual([]);
    await git.undo(res.undo!.id);
    expect((await git.log(1))[0].oid).toBe(old.oid);
    expect((await git.status()).files.map((f) => f.path)).toEqual(["health.yml"]);
  });

  it("refuses a published commit, an empty message and nothing to change", async () => {
    const { b, git } = await repo();
    expect(await reason(git, { action: "amend", message: "nope" })).toBe("published");
    await edit(b, "health.yml", "https://example.test/local");
    await git.commit("local", ["health.yml"]);
    expect(await reason(git, { action: "amend", message: "  " })).toBe("invalid-name");
    expect(await reason(git, { action: "amend" })).toBe("nothing-to-do");
  });
});

describe("restore and rewrite", () => {
  it("restores files from an earlier commit with a backup", async () => {
    const { b, git } = await repo();
    const before = (await b.getRequest("health.yml")).url;
    await edit(b, "health.yml", "https://example.test/second");
    await git.commit("second", ["health.yml"]);
    const p = await git.plan({ action: "restore-files", rev: "HEAD~1", paths: ["/health.yml"] });
    expect(p.files).toEqual(["/health.yml"]);
    const res = await git.apply(p.action);
    expect((await b.getRequest("health.yml")).url).toBe(before);
    await git.undo(res.undo!.id);
    expect((await b.getRequest("health.yml")).url).toBe("https://example.test/second");
    expect(await reason(git, { action: "restore-files", rev: "HEAD", paths: ["/health.yml"] })).toBe("nothing-to-do");
  });

  it("rewrites the checked out branch to a tip built with the building blocks", async () => {
    const { b, git } = await repo();
    await edit(b, "health.yml", "https://example.test/second");
    const c2 = await git.commit("second", ["health.yml"]);
    const first = (await git.log(2))[1].oid;
    const tree = await git.writeTree({ base: first, files: [] });
    const tip = await git.writeCommit({ tree, parents: [first], message: "squashed\n" });
    expect((await git.commitInfo(tip)).message).toBe("squashed\n");
    const action: GitAction = { action: "rewrite", branch: "main", tip, summary: "Dropped second" };
    const res = await git.apply(action);
    expect(res.message).toBe("Dropped second.");
    expect((await git.log(1))[0].oid).toBe(tip);
    await git.undo(res.undo!.id);
    expect((await git.log(1))[0].oid).toBe(c2.oid);
  });

  it("restores a deleted branch", async () => {
    const { git } = await repo();
    await git.apply({ action: "create-branch", name: "feature", start: "HEAD", switch: false });
    const tip = (await git.log(1))[0].oid;
    await git.apply({ action: "delete-branch", name: "feature", force: true });
    await git.apply({ action: "restore-branch", name: "feature", target: tip });
    expect((await git.branches()).some((x) => x.name === "feature")).toBe(true);
    expect(await reason(git, { action: "restore-branch", name: "main", target: tip })).toBe("current-branch");
  });
});

describe("parking only some files", () => {
  it("parks the named files and leaves the other changes", async () => {
    const { b, git } = await repo();
    await edit(b, "health.yml", "https://example.test/h");
    await edit(b, "users/list-users.yml", "https://example.test/u");
    await git.apply({ action: "stash", paths: ["/health.yml"] });
    expect((await git.status()).files.map((f) => f.path)).toEqual(["users/list-users.yml"]);
    expect((await git.stashes())[0].files).toEqual(["/health.yml"]);
    expect(await reason(git, { action: "stash", paths: ["/nothing.yml"] })).toBe("nothing-to-do");
  });
});

describe("abort", () => {
  it("stops a conflicted merge and undo puts the conflict back", async () => {
    const { b, git } = await repo();
    expect(await reason(git, { action: "abort" })).toBe("nothing-to-do");
    await git.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await edit(b, "health.yml", "https://example.test/theirs");
    await git.commit("theirs", ["health.yml"]);
    await git.apply({ action: "switch", target: "main" });
    await edit(b, "health.yml", "https://example.test/ours");
    await git.commit("ours", ["health.yml"]);
    await git.apply({ action: "merge", from: "other" });
    expect((await git.status()).merging).toBe(true);
    const head = (await git.log(1))[0].oid;
    const p = await git.plan({ action: "abort" });
    expect(p.blocked).toBeUndefined();
    const res = await git.apply({ action: "abort" });
    expect((await git.status()).merging).toBe(false);
    expect((await git.log(1))[0].oid).toBe(head);
    await git.undo(res.undo!.id);
    expect((await git.status()).merging).toBe(true);
  });
});
