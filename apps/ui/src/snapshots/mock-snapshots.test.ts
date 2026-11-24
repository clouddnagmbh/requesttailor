// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";

const input = { environment: "Local", vars: [], secrets: [] };

async function open() {
  const b = new MockBackend();
  await b.openCollection();
  return { b, path: "health.yml" };
}

describe("mock snapshots", () => {
  it("freezes, compares, ignores, accepts and stages unsaved files", async () => {
    const { b, path } = await open();
    const sent = await b.send(path, input);
    const id = sent.response_id!;
    expect((await b.snapshots.check(path, id)).state).toBe("none");
    expect(await b.snapshots.read(path)).toBeNull();

    const frozen = await b.snapshots.freeze(path, id);
    expect(frozen.assertion_added).toBe(true);
    expect(frozen.snapshot.endsWith(".snap.json")).toBe(true);
    expect((await b.getRequest(path)).assertions.at(-1)).toMatchObject({ expression: "res", operator: "matchesSnapshot" });
    const dirty = await b.dirty();
    expect(dirty).toContain(frozen.snapshot);
    expect(dirty).toContain(path);
    expect((await b.snapshots.list()).map((e) => e.request_path)).toContain(path);
    expect((await b.snapshots.check(path, id)).state).toBe("matches");

    // The next send carries the result of the assertion.
    const again = await b.send(path, input);
    const result = again.report.assertions?.find((a) => a.operator === "matchesSnapshot");
    expect(result?.passed).toBe(true);

    // Freezing again keeps the assertion and the ignored paths.
    const snap = await b.snapshots.setIgnore(path, ["$.volatile", "$.volatile"]);
    expect(snap.ignore).toEqual(["$.volatile"]);
    const again2 = await b.snapshots.freeze(path, id);
    expect(again2.assertion_added).toBe(false);
    expect(again2.content.ignore).toEqual(["$.volatile"]);
    await expect(b.snapshots.setIgnore(path, ["nonsense"])).rejects.toThrow(/ignore path/);

    const saved = await b.save();
    expect(saved.map((c) => c.path)).toContain(frozen.snapshot);
  });

  it("a missing file fails the assertion with the hint", async () => {
    const { b, path } = await open();
    const spec = await b.getRequest(path);
    spec.assertions.push({ expression: "res", operator: "matchesSnapshot", value: "nope.snap.json", description: null, enabled: true });
    await b.updateRequest(path, spec);
    const sent = await b.send(path, input);
    const a = sent.report.assertions!.find((x) => x.operator === "matchesSnapshot")!;
    expect(a.passed).toBe(false);
    expect(a.error).toBe("no snapshot yet; run `rt snapshot update`");
    expect((await b.snapshots.check(path, sent.response_id!)).state).toBe("missing");
  });
});
