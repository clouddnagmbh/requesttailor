// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: the mock's revisions (`revParse`, `treeAt`, `show`, `fileLog`,
// `mergeBase`, `commits`) and the placeholder APIs of the strands.

import { describe, expect, it } from "vitest";

import { BackendError } from "./backend";
import { MockBackend } from "./mock";
import type { MockGit } from "./mock-git";

async function demo() {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  return { b, git };
}

describe("mock revisions", () => {
  it("reads commits without a checkout", async () => {
    const { b, git } = await demo();
    const first = await git.revParse("HEAD");
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, url: "{{baseUrl}}/up" });
    await b.save();
    const second = await git.commit("Health moved", ["health.yml"]);

    expect(await git.revParse("HEAD")).toBe(second.oid);
    expect(await git.revParse("HEAD~1")).toBe(first);
    expect(await git.revParse("main^")).toBe(first);
    expect(await git.revParse(second.oid.slice(0, 8))).toBe(second.oid);
    await expect(git.revParse("nope")).rejects.toThrow(/Unknown revision/);

    const tree = await git.treeAt(first);
    expect(tree.oid).toBe(first);
    expect(tree.files["health.yml"]).toContain("{{baseUrl}}/health");
    expect(await git.show("HEAD", "health.yml")).toContain("{{baseUrl}}/up");
    expect(await git.show("HEAD", "missing.yml")).toBeNull();
    // The model of a request at a revision (a copy), for mocks that work on models.
    expect(git.specAt(first, "health.yml")?.url).toBe("{{baseUrl}}/health");
    expect(git.specAt("HEAD", "/health.yml")?.url).toBe("{{baseUrl}}/up");
    expect(git.specAt("HEAD", "missing.yml")).toBeNull();
    git.specAt("HEAD", "health.yml")!.url = "changed";
    expect(git.specAt("HEAD", "health.yml")?.url).toBe("{{baseUrl}}/up");

    expect(await git.mergeBase("HEAD", first)).toBe(first);
    expect((await git.commits(first, "HEAD", 10)).map((c) => c.oid)).toEqual([second.oid]);
    expect((await git.commits(null, "HEAD", 10)).map((c) => c.oid)).toEqual([second.oid, first]);

    const log = await git.fileLog("health.yml", 10);
    expect(log.map((e) => [e.commit.oid, e.change])).toEqual([
      [second.oid, "modified"],
      [first, "added"],
    ]);
  });

  it("wave 12 APIs refuse with unsupported until their strand lands", async () => {
    const { b } = await demo();
    await expect(b.merges.resolve([])).rejects.toBeInstanceOf(BackendError);
    expect(await b.merges.conflicts()).toEqual({ merging: false, conflicts: [], text_conflicts: [] });
    expect(await b.review.repo()).toBeNull();
    expect(await b.contract.spec()).toBeNull();
    expect(await b.timeline.results("HEAD")).toEqual([]);
  });
});
