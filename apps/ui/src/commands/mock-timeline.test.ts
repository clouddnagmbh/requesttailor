// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock's request history (wave 12, strand D): versions with their field changes, blame per
// field, an old version, and results per commit (at most 20 per request).

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import type { MockGit } from "./mock-git";
import type { MockTimeline } from "./mock-timeline";

async function demo() {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  const first = await git.revParse("HEAD");
  const spec = await b.getRequest("health.yml");
  await b.updateRequest("health.yml", { ...spec, url: "{{baseUrl}}/up" });
  await b.save();
  const second = await git.commit("Health moved", ["health.yml"]);
  await b.updateRequest("health.yml", {
    ...spec,
    url: "{{baseUrl}}/up",
    headers: [{ name: "X-Trace", value: "1", enabled: true }],
  });
  await b.save();
  const third = await git.commit("Trace header", ["health.yml"]);
  return { b, timeline: b.timeline as MockTimeline, first, second, third };
}

describe("mock timeline", () => {
  it("lists the versions with what each commit changed", async () => {
    const { timeline, first, second, third } = await demo();
    const t = await timeline.history("health.yml", 10);
    expect(t.entries.map((e) => e.commit.commit.oid)).toEqual([third.oid, second.oid, first]);
    expect(t.entries[0].fields.map((f) => f.path.join("/"))).toEqual(["headers/X-Trace"]);
    expect(t.entries[1].fields.map((f) => [f.path.join("/"), f.old, f.new])).toEqual([
      ["url", { kind: "text", text: "{{baseUrl}}/health" }, { kind: "text", text: "{{baseUrl}}/up" }],
    ]);
    expect(t.entries[2].commit.change).toBe("added");
    expect(t.entries[2].fields.map((f) => f.path.join("/"))).toEqual(["name", "method", "url"]);
  });

  it("blames each field on the commit that last changed it", async () => {
    const { timeline, first, second, third } = await demo();
    const by = new Map((await timeline.blame("health.yml")).map((b) => [b.field.join("/"), b.commit.oid]));
    expect(by.get("name")).toBe(first);
    expect(by.get("url")).toBe(second.oid);
    expect(by.get("headers/X-Trace")).toBe(third.oid);
  });

  it("reads an old version", async () => {
    const { timeline, first } = await demo();
    expect((await timeline.version(first, "health.yml")).url).toBe("{{baseUrl}}/health");
    await expect(timeline.version(first, "nope.yml")).rejects.toThrow(/does not exist/);
  });

  it("keeps results per commit and request, at most 20", async () => {
    const { timeline, second } = await demo();
    for (let i = 0; i < 25; i++) await timeline.record(second.oid, { request: "health.yml", time: i });
    await timeline.record(second.oid, { request: "other.yml", time: 99 });
    const all = await timeline.results(second.oid);
    expect(all.filter((r) => r.request === "health.yml")).toHaveLength(20);
    expect(all[0].time).toBe(99);
    const t = await timeline.history("health.yml", 10);
    expect(t.entries[1].tests).toHaveLength(20);
    expect(t.entries[1].tests[0].time).toBe(24);
    expect(await timeline.results("HEAD")).toEqual([]);
  });
});
