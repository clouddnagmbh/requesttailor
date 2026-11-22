// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock's merge per field (wave 12, strand B): what the merge view sees after a pull the
// mock Git stopped, and what `resolve` writes.

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import type { MockGit } from "./mock-git";

async function pulled(theirs: (spec: Awaited<ReturnType<MockBackend["getRequest"]>>) => object, ours: (spec: Awaited<ReturnType<MockBackend["getRequest"]>>) => object) {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  const base = await b.getRequest("health.yml");
  git.remoteCommit({ "health.yml": { ...base, ...theirs(base) } }, "Colleague: health");
  await b.updateRequest("health.yml", { ...base, ...ours(base) });
  await b.save();
  await git.commit("Mine: health", ["health.yml"]);
  expect((await git.pull()).outcome).toBe("conflicts");
  return { b, git, base };
}

describe("mock merges", () => {
  it("finds one conflict per field and leaves other fields merged", async () => {
    const { b, git } = await pulled(
      () => ({ method: "POST", url: "{{baseUrl}}/theirs" }),
      () => ({ url: "{{baseUrl}}/ours" }),
    );
    const state = await b.merges.conflicts();
    expect(state.merging).toBe(true);
    expect(state.text_conflicts).toEqual([]);
    expect(state.conflicts).toHaveLength(1);
    expect(state.conflicts[0]).toMatchObject({
      target: { kind: "request", path: "health.yml" },
      file: "health.yml",
      field: ["url"],
      ours: { kind: "text", text: "{{baseUrl}}/ours" },
      theirs: { kind: "text", text: "{{baseUrl}}/theirs" },
    });

    // Nothing is decided yet: resolve without choices writes nothing.
    await b.merges.resolve([]);
    expect((await b.merges.conflicts()).conflicts).toHaveLength(1);

    const c = state.conflicts[0];
    await b.merges.resolve([{ kind: "field", target: c.target, field: c.field, side: "theirs" }]);
    const merged = await b.getRequest("health.yml");
    expect([merged.method, merged.url]).toEqual(["POST", "{{baseUrl}}/theirs"]);
    expect(await b.merges.conflicts()).toEqual({ merging: true, conflicts: [], text_conflicts: [] });
    await git.commit("Merge", ["health.yml"]);
    expect((await git.status()).merging).toBe(false);
  });

  it("lists a request with changes in different fields as clean and resolves it", async () => {
    const { b } = await pulled(
      (base) => ({ headers: [...base.headers, { name: "X-Theirs", value: "1", enabled: true }] }),
      () => ({ url: "{{baseUrl}}/ours" }),
    );
    expect(await b.merges.conflicts()).toEqual({ merging: true, conflicts: [], text_conflicts: [], clean: ["health.yml"] });
    await b.merges.resolve([]);
    const merged = await b.getRequest("health.yml");
    expect(merged.url).toBe("{{baseUrl}}/ours");
    expect(merged.headers.map((h) => h.name)).toContain("X-Theirs");
  });
});
