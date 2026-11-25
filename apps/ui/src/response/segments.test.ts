// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand A: the segment tree and the contributions of extensions.

import { describe, expect, it } from "vitest";

import { SegmentContributions, layoutSegments, overWaitHint } from "./segments";

const seg = (id: string, ms: number, parent?: string, offset_ms?: number) => ({ id, label: id, ms, parent, offset_ms });

describe("layoutSegments", () => {
  it("nests children under their parent, in input order, with offsets", () => {
    const l = layoutSegments([seg("gw", 40, "server", 0), seg("app", 30, "gw", 5), seg("db", 12, "app", 8), seg("cache", 3, "gw", 36)]);
    expect(l.rows.map((r) => [r.segment.id, r.depth, r.start, r.end])).toEqual([
      ["gw", 0, 0, 40],
      ["app", 1, 5, 35],
      ["db", 2, 8, 20],
      ["cache", 1, 36, 39],
    ]);
    expect(l.span).toBe(40);
    expect(l.top).toBe(40);
  });

  it("puts a segment without offset after its previous sibling, or at its parent's start", () => {
    const l = layoutSegments([seg("a", 10), seg("b", 5), seg("a1", 4, "a"), seg("a2", 3, "a")]);
    const at = (id: string) => l.rows.find((r) => r.segment.id === id)!;
    expect([at("a").start, at("b").start]).toEqual([0, 10]);
    expect([at("a1").start, at("a2").start]).toEqual([0, 4]);
  });

  it("treats an unknown parent as top-level and survives cycles and self references", () => {
    const l = layoutSegments([seg("x", 5, "nope"), seg("c1", 2, "c2"), seg("c2", 2, "c1"), seg("me", 1, "me")]);
    expect(l.rows).toHaveLength(4);
    expect(l.rows.every((r) => r.depth === 0)).toBe(true);
  });

  it("copes with an empty set and zero lengths", () => {
    expect(layoutSegments([]).rows).toEqual([]);
    expect(layoutSegments([seg("z", 0)]).span).toBe(1);
  });

  it("warns when the top-level segments are longer than the wait", () => {
    const l = layoutSegments([seg("gw", 400)]);
    expect(overWaitHint(l, 82)).toMatch(/Longer than the 82 ms wait/);
    expect(overWaitHint(l, 400)).toBeNull();
  });
});

describe("SegmentContributions", () => {
  it("replaces per source, keeps sources apart and notifies", () => {
    const c = new SegmentContributions();
    let n = 0;
    c.subscribe(() => n++);
    c.contribute(1, "trace", [seg("a", 1)]);
    c.contribute(1, "other", [seg("b", 2)]);
    c.contribute(1, "trace", [seg("c", 3)]);
    expect(c.forResponse(1).map((s) => [s.source, s.segments[0]!.id])).toEqual([
      ["trace", "c"],
      ["other", "b"],
    ]);
    c.contribute(1, "other", []);
    expect(c.forResponse(1)).toHaveLength(1);
    expect(c.forResponse(2)).toEqual([]);
    expect(n).toBe(4);
  });
});
