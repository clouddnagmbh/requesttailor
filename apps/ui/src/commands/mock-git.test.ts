// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { lineDiff, MockGit, MOCK_REMOTE_HOST } from "./mock-git";

const kinds = (d: ReturnType<typeof lineDiff>) => d.hunks.flatMap((h) => h.lines.map((l) => `${l.kind[0]} ${l.text}`));

describe("lineDiff (same cases as rt_app::git::text_diff)", () => {
  it("one changed line with context", () => {
    const d = lineDiff("x.yml", "a\nb\nc\nd\ne\nf\ng\nh\n", "a\nb\nc\nd\nE\nf\ng\nh\n");
    expect(d.hunks).toHaveLength(1);
    const h = d.hunks[0];
    expect([h.old_start, h.old_lines, h.new_start, h.new_lines]).toEqual([2, 7, 2, 7]);
    expect(kinds(d)).toEqual(["c b", "c c", "c d", "r e", "a E", "c f", "c g", "c h"]);
  });

  it("new and deleted files", () => {
    const h = lineDiff("n.yml", null, "x\ny\n").hunks[0];
    expect([h.old_start, h.old_lines, h.new_start, h.new_lines]).toEqual([0, 0, 1, 2]);
    expect(kinds(lineDiff("n.yml", "x\n", null))).toEqual(["r x"]);
    expect(lineDiff("same", "a\n", "a\n").hunks).toEqual([]);
  });

  it("splits far-apart changes into hunks", () => {
    const old = Array.from({ length: 20 }, (_, i) => `l${i}`).join("\n");
    const now = old.replace("l1\n", "L1\n").replace("l18", "L18");
    expect(lineDiff("f", old, now).hunks).toHaveLength(2);
  });
});

describe("MockGit", () => {
  async function demo() {
    const b = new MockBackend();
    await b.openCollection();
    const git = b.git as MockGit;
    await git.setIdentity({ name: "Ada", email: "ada@example.com" });
    return { b, git };
  }

  async function editAndSave(b: MockBackend, path: string, url: string) {
    const spec = await b.getRequest(path);
    await b.updateRequest(path, { ...spec, url });
    await b.save();
  }

  it("starts clean, in sync with origin", async () => {
    const { git } = await demo();
    expect(await git.status()).toEqual({
      repo: true,
      branch: "main",
      upstream: "origin/main",
      ahead: 0,
      behind: 0,
      files: [],
      merging: false,
    });
    expect((await git.log(10)).map((c) => c.summary)).toEqual(["Initial collection"]);
  });

  it("commits selected files, pushes and pulls", async () => {
    const { b, git } = await demo();
    await editAndSave(b, "health.yml", "{{baseUrl}}/up");
    await editAndSave(b, "users/list-users.yml", "{{baseUrl}}/people");
    expect((await git.status()).files.map((f) => f.path)).toEqual(["health.yml", "users/list-users.yml"]);
    const diff = await git.diff("health.yml");
    expect(diff.hunks[0].lines.filter((l) => l.kind !== "context").map((l) => l.text.trim())).toEqual([
      '"url": "{{baseUrl}}/health",',
      '"url": "{{baseUrl}}/up",',
    ]);
    await git.commit("Health moved", ["health.yml"]);
    expect(await git.status()).toMatchObject({ ahead: 1, files: [{ path: "users/list-users.yml" }] });
    await git.push();
    expect((await git.status()).ahead).toBe(0);

    git.remoteCommit({ "health.yml": { ...(await b.getRequest("health.yml")), url: "{{baseUrl}}/remote" } });
    await git.commit("people", ["users/list-users.yml"]);
    await expect(git.push()).rejects.toThrow(/Pull first/);
    expect(await git.pull()).toMatchObject({ outcome: "merged" });
    expect((await b.getRequest("health.yml")).url).toBe("{{baseUrl}}/remote");
    await git.push();
    expect(await git.pull()).toEqual({ outcome: "up-to-date" });
  });

  it("refuses to switch with unsaved changes; switching changes the requests", async () => {
    const { b, git } = await demo();
    await git.createBranch("feature", true);
    await b.createRequest("", "Feature only");
    await expect(git.checkout("main")).rejects.toThrow(/Save your changes/);
    await b.save();
    await git.commit("feature request", ["feature-only.yml"]);
    await git.checkout("main");
    expect((await b.tree()).kind === "folder" && JSON.stringify(await b.tree())).not.toContain("Feature only");
    await git.checkout("feature");
    expect(JSON.stringify(await b.tree())).toContain("Feature only");
  });

  it("scripted conflict: pull stops, resolve and commit finish the merge", async () => {
    const { b, git } = await demo();
    git.scenario("conflict");
    expect(await git.status()).toMatchObject({ ahead: 1, behind: 0 });
    expect(await git.pull()).toEqual({ outcome: "conflicts", paths: ["health.yml"] });
    expect(await git.status()).toMatchObject({ merging: true, files: [{ path: "health.yml", state: "conflicted" }] });
    const c = await git.conflict("health.yml");
    expect(c.ours).toContain("/health/local");
    expect(c.theirs).toContain("/health/remote");
    expect(c.base).toContain('"{{baseUrl}}/health"');
    await git.resolve("health.yml", "theirs");
    expect((await b.getRequest("health.yml")).url).toBe("{{baseUrl}}/health/remote");
    await git.commit("Merge origin/main into main", ["health.yml"]);
    expect(await git.status()).toMatchObject({ merging: false, files: [], ahead: 2, behind: 0 });
  });

  it("asks for credentials when the host requires them", async () => {
    const { git } = await demo();
    git.requireAuth();
    await expect(git.push()).rejects.toMatchObject({ kind: "git-auth", message: MOCK_REMOTE_HOST });
    await git.setCredentials(MOCK_REMOTE_HOST, "ada", "pw");
    expect(await git.hasCredentials(MOCK_REMOTE_HOST)).toBe(true);
    await git.push();
  });

  it("init, then clone opens the collection", async () => {
    const { git } = await demo();
    git.scenario("none");
    expect((await git.status()).repo).toBe(false);
    await git.init();
    expect(await git.status()).toMatchObject({ repo: true, branch: null, upstream: null });
    expect((await git.status()).files.length).toBeGreaterThan(0);
    git.scenario("none");
    expect(await git.clone("https://git.example.com/acme/demo-api.git")).toMatchObject({ name: "Demo API" });
    expect((await git.status()).repo).toBe(true);
  });
});
