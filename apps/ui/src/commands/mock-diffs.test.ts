// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand A: the mock's semantic diff (same shape as `rt_format::diff`).

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import type { MockGit } from "./mock-git";

async function demo() {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  return { b, git };
}

describe("mock diffs", () => {
  it("shows saved changes per field and nothing when clean", async () => {
    const { b } = await demo();
    expect((await b.diffs.working()).changes).toEqual([]);
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, url: "{{baseUrl}}/up", headers: [...spec.headers, { name: "X-Trace", value: "1", enabled: true }] });
    expect((await b.diffs.working()).changes, "unsaved edits are not part of it").toEqual([]);
    await b.save();
    const d = await b.diffs.working();
    expect(d.changes).toHaveLength(1);
    const c = d.changes[0];
    expect(c.target).toEqual({ kind: "request", path: "health.yml" });
    expect(c.change).toEqual({ kind: "modified" });
    expect(c.fields.map((f) => f.path.join("/"))).toEqual(["url", "headers/X-Trace"]);
    expect(c.fields[0]).toMatchObject({ old: { kind: "text", text: "{{baseUrl}}/health" }, new: { kind: "text", text: "{{baseUrl}}/up" } });
    expect(c.fields[0].line).toBeGreaterThan(0);
    expect(d.files).toEqual([{ path: "health.yml", status: "modified", covered: true }]);
  });

  it("diffs a commit against its parent and a revision pair, with moves and added files", async () => {
    const { b, git } = await demo();
    const first = await git.revParse("HEAD");
    const spec = await b.getRequest("health.yml");
    const id = await b.createRequest("", "Ping");
    await b.updateRequest("health.yml", { ...spec, method: "POST" });
    const paths = await b.dirty();
    await b.save();
    const second = await git.commit("More", paths);
    const d = await b.diffs.commit(second.oid);
    expect(d.changes.map((c) => [c.name, c.change.kind])).toContainEqual(["Health", "modified"]);
    expect(d.changes.map((c) => c.change.kind)).toContain("added");
    expect(await b.diffs.between(first, second.oid)).toEqual(d);
    expect(id).toBeTruthy();
    // The root commit is all additions.
    const root = await b.diffs.commit(first);
    expect(root.changes.every((c) => c.change.kind === "added")).toBe(true);
  });
});
