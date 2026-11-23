// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: the mock's wizard actions follow the rules of every shell (the same case
// list as crates/rt-git/tests/{actions,undo,stash}.rs and apps/web/src/git/actions.test.ts):
// plan changes nothing, apply refuses what is blocked, every destructive action has a backup
// and an undo that puts refs and files back, and undo refuses once refs moved.

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import type { MockGit, MockGitScenario } from "./mock-git";
import type { GitAction, GitBlockReason } from "./types";

async function repo(scenario: MockGitScenario = "clean") {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  git.scenario(scenario);
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  return { b, git };
}

async function edit(b: MockBackend, path: string, url: string, save = true) {
  const spec = await b.getRequest(path);
  await b.updateRequest(path, { ...spec, url });
  if (save) await b.save();
}

/** Everything a user can see of the repository, to prove that plan changes nothing and undo
 *  restores it. */
async function state(git: MockGit) {
  return JSON.stringify({
    status: await git.status(),
    branches: await git.branches(),
    log: await git.log(50),
    stashes: await git.stashes(),
    remotes: await git.remotes(),
    tree: (await git.status()).files.length ? await git.treeAt("HEAD") : null,
  });
}

async function workFiles(b: MockBackend) {
  const out: Record<string, string> = {};
  for (const p of ["health.yml", "users/list-users.yml", "users/create-user.yml", "demo/blocked.yml"]) {
    out[p] = await b.getRequest(p).then(
      (s) => s.url,
      () => "(none)",
    );
  }
  return out;
}

const blocked = async (git: MockGit, action: GitAction) => (await git.plan(action)).blocked?.reason;

describe("plan only reads", () => {
  it("leaves branches, files and history as they are for every action", async () => {
    const { b, git } = await repo("showcase");
    const before = await state(git);
    const files = await workFiles(b);
    const head = await git.revParse("HEAD");
    const actions: GitAction[] = [
      { action: "create-branch", name: "x", start: "HEAD", switch: true },
      { action: "switch", target: "feature/auth" },
      { action: "rename-branch", from: "feature/auth", to: "y" },
      { action: "delete-branch", name: "feature/auth", force: true },
      { action: "merge", from: "feature/auth" },
      { action: "stash" },
      { action: "revert", commit: head },
      { action: "reset", target: "HEAD~1", mode: "discard" },
      { action: "discard", paths: [] },
      { action: "create-tag", name: "v2", target: "HEAD" },
      { action: "delete-tag", name: "v1.0" },
      { action: "add-remote", name: "backup", url: "https://example.com/x.git" },
    ];
    for (const a of actions) await git.plan(a);
    expect(await state(git)).toBe(before);
    expect(await workFiles(b)).toEqual(files);
  });
});

describe("blocked, with the reason and plain words", () => {
  it("names each reason", async () => {
    const { b, git } = await repo("clean");
    const cases: [GitAction, GitBlockReason][] = [
      [{ action: "create-branch", name: "my branch", start: "HEAD" }, "invalid-name"],
      [{ action: "create-branch", name: "main", start: "HEAD" }, "exists"],
      [{ action: "create-branch", name: "x", start: "nope" }, "not-found"],
      [{ action: "delete-branch", name: "main" }, "current-branch"],
      [{ action: "merge", from: "main" }, "nothing-to-do"],
      [{ action: "push" }, "nothing-to-do"],
      [{ action: "stash" }, "nothing-to-do"],
    ];
    for (const [a, reason] of cases) expect(await blocked(git, a), a.action).toBe(reason);

    await edit(b, "health.yml", "{{baseUrl}}/unsaved", false);
    expect(await blocked(git, { action: "switch", target: "HEAD~0" })).toBe("unsaved-changes");
    await b.save();
    await git.createBranch("side", false);
    await git.apply({ action: "create-branch", name: "other", start: "HEAD" });
    expect(await blocked(git, { action: "merge", from: "other" })).toBe("nothing-to-do");

    const fresh = await repo("unpublished");
    expect(await blocked(fresh.git, { action: "push" })).toBe("no-upstream");
    expect(await blocked(fresh.git, { action: "pull" })).toBe("no-upstream");
    expect(await blocked((await repo("diverged")).git, { action: "push" })).toBe("diverged");
    expect(await blocked((await repo("behind")).git, { action: "push" })).toBe("rejected");
    expect(await blocked((await repo("detached")).git, { action: "merge", from: "main" })).toBe("detached");
  });

  it("apply refuses what plan blocks, with the plan's message", async () => {
    const { git } = await repo();
    await expect(git.apply({ action: "create-branch", name: "main", start: "HEAD" })).rejects.toMatchObject({
      kind: "git",
      message: expect.stringContaining("exists already"),
    });
  });

  it("local changes block merging and switching to a branch that changes them", async () => {
    const { b, git } = await repo("showcase");
    expect(await blocked(git, { action: "merge", from: "origin/feature/auth" })).toBe("nothing-to-do");
    await git.apply({ action: "create-branch", name: "side", start: "HEAD~3" });
    expect(await blocked(git, { action: "merge", from: "side" })).toBe("nothing-to-do");
    await edit(b, "users/create-user.yml", "{{baseUrl}}/changed");
    expect(await blocked(git, { action: "switch", target: "side" })).toBe("local-changes");
  });
});

describe("the beginner's way: branch, commit, merge back, push", () => {
  it("works only with plan and apply", async () => {
    const { b, git } = await repo();
    let p = await git.plan({ action: "create-branch", name: "feature/login", start: "HEAD", switch: true });
    expect(p.blocked).toBeUndefined();
    expect(p.steps[0]).toContain("Creates the branch feature/login");
    await git.apply(p.action);
    expect((await git.status()).branch).toBe("feature/login");

    await edit(b, "health.yml", "{{baseUrl}}/login-health");
    await git.commit("Login health", ["health.yml"]);
    await git.apply({ action: "switch", target: "main" });
    expect((await b.getRequest("health.yml")).url).toBe("{{baseUrl}}/health");

    p = await git.plan({ action: "merge", from: "feature/login" });
    expect(p.merge).toMatchObject({ fast_forward: true, commits: [{ summary: "Login health" }] });
    const r = await git.apply(p.action);
    expect(r.reload).toBe(true);
    expect((await b.getRequest("health.yml")).url).toBe("{{baseUrl}}/login-health");
    expect((await git.status()).ahead).toBe(1);
    await git.apply({ action: "push" });
    expect((await git.status()).ahead).toBe(0);
  });

  it("publishes a new branch and fetches", async () => {
    const { git } = await repo("unpublished");
    const p = await git.plan({ action: "publish", branch: "feature/login", remote: "origin" });
    expect(p).toMatchObject({ network: true, undoable: false });
    await git.apply(p.action);
    expect((await git.status()).upstream).toBe("origin/feature/login");
    git.remoteCommit({}, "Colleague", "feature/login");
    expect((await git.status()).behind).toBe(0);
    await git.apply({ action: "fetch" });
    expect((await git.status()).behind).toBe(1);
  });
});

describe("merges field by field", () => {
  it("merges different fields of the same request without a conflict", async () => {
    const { b, git } = await repo();
    await git.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, method: "POST" });
    await b.save();
    await git.commit("POST", ["health.yml"]);
    await git.apply({ action: "switch", target: "main" });
    await edit(b, "health.yml", "{{baseUrl}}/v2/health");
    await git.commit("v2", ["health.yml"]);
    const p = await git.plan({ action: "merge", from: "other" });
    expect(p.merge?.fast_forward).toBe(false);
    expect(p.merge?.files).toEqual([{ path: "/health.yml", semantic: true, clean: 1, conflicts: [] }]);
    await git.apply(p.action);
    const merged = await b.getRequest("health.yml");
    expect([merged.method, merged.url]).toEqual(["POST", "{{baseUrl}}/v2/health"]);
    expect((await git.log(1))[0].summary).toBe("Merge other into main");
  });

  it("stops with a conflict when both sides changed the same field, and shows it in the plan", async () => {
    const { b, git } = await repo();
    await git.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await edit(b, "health.yml", "{{baseUrl}}/theirs");
    await git.commit("theirs", ["health.yml"]);
    await git.apply({ action: "switch", target: "main" });
    await edit(b, "health.yml", "{{baseUrl}}/ours");
    await git.commit("ours", ["health.yml"]);
    const p = await git.plan({ action: "merge", from: "other" });
    expect(p.merge?.files[0].conflicts).toEqual(["url"]);
    const r = await git.apply(p.action);
    expect(r.outcome).toEqual({ outcome: "conflicts", paths: ["health.yml"] });
    expect((await git.status()).merging).toBe(true);
    await git.resolve("health.yml", "theirs");
    await git.commit("Merge other into main", ["health.yml"]);
    expect((await git.status()).merging).toBe(false);
  });

  it("cherry-picks and reverts as new commits, semantically", async () => {
    const { b, git } = await repo();
    await git.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await edit(b, "users/list-users.yml", "{{baseUrl}}/users?page=2");
    const picked = await git.commit("Page 2", ["users/list-users.yml"]);
    await git.apply({ action: "switch", target: "main" });
    await edit(b, "health.yml", "{{baseUrl}}/mine");
    await git.commit("Mine", ["health.yml"]);

    const p = await git.plan({ action: "cherry-pick", commit: picked.oid });
    expect(p.merge?.files).toEqual([{ path: "/users/list-users.yml", semantic: true, clean: 1, conflicts: [] }]);
    await git.apply(p.action);
    expect((await b.getRequest("users/list-users.yml")).url).toBe("{{baseUrl}}/users?page=2");
    const log = await git.log(3);
    expect(log[0].summary).toBe("Page 2");

    await git.apply({ action: "revert", commit: log[1].oid });
    expect((await b.getRequest("health.yml")).url).toBe("{{baseUrl}}/health");
    expect((await git.log(1))[0].summary).toBe('Revert "Mine"');
    expect(await blocked(git, { action: "cherry-pick", commit: picked.oid })).toBe("nothing-to-do");
  });
});

describe("backup and undo", () => {
  const destructive: [string, MockGitScenario, (git: MockGit, b: MockBackend) => Promise<GitAction>][] = [
    ["reset", "showcase", async () => ({ action: "reset", target: "HEAD~2", mode: "discard" })],
    ["discard", "showcase", async () => ({ action: "discard", paths: [] })],
    [
      "delete-branch",
      "showcase",
      async (git) => {
        await git.apply({ action: "create-branch", name: "doomed", start: "HEAD" });
        return { action: "delete-branch", name: "feature/auth", force: true };
      },
    ],
    [
      "merge",
      "clean",
      async (git, b) => {
        await git.apply({ action: "create-branch", name: "o", start: "HEAD", switch: true });
        await edit(b, "health.yml", "{{baseUrl}}/o");
        await git.commit("o", ["health.yml"]);
        await git.apply({ action: "switch", target: "main" });
        await edit(b, "users/list-users.yml", "{{baseUrl}}/m");
        await git.commit("m", ["users/list-users.yml"]);
        return { action: "merge", from: "o" };
      },
    ],
    [
      "cherry-pick",
      "clean",
      async (git, b) => {
        await git.apply({ action: "create-branch", name: "o", start: "HEAD", switch: true });
        await edit(b, "health.yml", "{{baseUrl}}/o");
        const c = await git.commit("o", ["health.yml"]);
        await git.apply({ action: "switch", target: "main" });
        return { action: "cherry-pick", commit: c.oid };
      },
    ],
    ["revert", "ahead", async (git) => ({ action: "revert", commit: await git.revParse("HEAD") })],
    [
      "unstash",
      "showcase",
      async (git) => {
        await git.apply({ action: "stash", message: "wip" });
        return { action: "unstash", index: 0 };
      },
    ],
    [
      "drop-stash",
      "showcase",
      async (git) => {
        await git.apply({ action: "stash", message: "wip" });
        return { action: "drop-stash", index: 0 };
      },
    ],
  ];

  for (const [name, scenario, setup] of destructive) {
    it(`${name}: backup ref, then undo restores refs and files`, async () => {
      const { b, git } = await repo(scenario);
      const action = await setup(git, b);
      const before = await state(git);
      const files = await workFiles(b);
      const plan = await git.plan(action);
      expect(plan.blocked, name).toBeUndefined();
      expect(plan.backup, name).toBe(true);
      expect(plan.undoable, name).toBe(true);
      const result = await git.apply(action);
      expect(result.undo, name).toBeDefined();
      const actions = (git as unknown as { wave14Actions: { backupRefs(): Map<string, string> } }).wave14Actions;
      for (const ref of result.undo!.backups ?? []) {
        expect(ref).toMatch(/^refs\/requesttailor\/backup\/\d+\//);
        expect(actions.backupRefs().has(ref)).toBe(true);
      }
      if (name !== "unstash") expect(result.undo!.backups?.length ?? 0, name).toBeGreaterThan(0);
      expect((await git.lastAction())?.id).toBe(result.undo!.id);
      await git.undo(result.undo!.id);
      expect(await state(git), name).toBe(before);
      expect(await workFiles(b), name).toEqual(files);
    });
  }

  it("undo is refused once the branch moved since", async () => {
    const { b, git } = await repo();
    const r = await git.apply({ action: "create-branch", name: "x", start: "HEAD", switch: true });
    await edit(b, "health.yml", "{{baseUrl}}/later");
    await git.commit("later", ["health.yml"]);
    const last = await git.lastAction();
    expect(last?.stale).toContain("changed since");
    await expect(git.undo(r.undo!.id)).rejects.toMatchObject({ kind: "git" });
  });

  it("pushes cannot be undone and say so", async () => {
    const { git } = await repo("ahead");
    const p = await git.plan({ action: "push" });
    expect(p.undoable).toBe(false);
    expect(p.steps.join(" ")).toContain("cannot be undone");
    const r = await git.apply(p.action);
    expect(r.undo).toBeUndefined();
  });
});

describe("parked changes", () => {
  it("parks and brings back, listed newest first", async () => {
    const { b, git } = await repo();
    await edit(b, "health.yml", "{{baseUrl}}/parked");
    await git.apply({ action: "stash", message: "try health" });
    expect((await git.status()).files).toEqual([]);
    expect((await b.getRequest("health.yml")).url).toBe("{{baseUrl}}/health");
    const list = await git.stashes();
    expect(list).toMatchObject([{ index: 0, message: "try health", branch: "main", files: ["/health.yml"] }]);
    await git.apply({ action: "unstash", index: 0 });
    expect((await b.getRequest("health.yml")).url).toBe("{{baseUrl}}/parked");
    expect(await git.stashes()).toEqual([]);
  });
});

describe("remotes and tags", () => {
  it("adds, renames, re-points and removes remotes; tags and pushes tags", async () => {
    const { git } = await repo("no-remote");
    expect(await git.remotes()).toEqual([]);
    await git.apply({ action: "add-remote", name: "origin", url: "https://git.example.com/acme/demo-api.git" });
    expect(await git.remotes()).toEqual([{ name: "origin", url: "https://git.example.com/acme/demo-api.git" }]);
    await git.apply({ action: "set-remote-url", name: "origin", url: "https://git.example.com/acme/api.git" });
    await git.apply({ action: "rename-remote", from: "origin", to: "upstream" });
    expect((await git.remotes()).map((r) => r.name)).toEqual(["upstream"]);
    await git.apply({ action: "rename-remote", from: "upstream", to: "origin" });
    await git.apply({ action: "publish", branch: "main", remote: "origin" });
    await git.apply({ action: "create-tag", name: "v1", target: "HEAD", message: "First" });
    expect(await git.revParse("v1")).toBe(await git.revParse("HEAD"));
    await git.apply({ action: "push-tag", name: "v1", remote: "origin" });
    expect(await blocked(git, { action: "push-tag", name: "v1", remote: "origin" })).toBe("nothing-to-do");
    const del = await git.apply({ action: "delete-tag", name: "v1" });
    await git.undo(del.undo!.id);
    expect(await git.revParse("v1")).toBe(await git.revParse("HEAD"));
  });
});

describe("detached HEAD", () => {
  it("creates a branch where you are, or goes back", async () => {
    const { git } = await repo("detached");
    expect((await git.status()).branch).toBeNull();
    await git.apply({ action: "create-branch", name: "rescue", start: "HEAD", switch: true });
    expect((await git.status()).branch).toBe("rescue");
    await git.apply({ action: "switch", target: "main" });
    expect((await git.log(1))[0].summary).toBe("Use the new health URL");
  });
});
