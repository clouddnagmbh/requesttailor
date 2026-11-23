// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { CollectionDiff, ModelChange, RequestComparison, RunComparison, SpecChange } from "../commands/types";
import { buildRows, filterRows, headerCounts, nextChange, rowSummary } from "./rows";

const commit = (oid: string) => ({ oid, summary: oid, author: "A", time: 0 });
const summary = { requests: 0, passed: 0, failed: 0, skipped: 0, tests_passed: 0, tests_failed: 0, assertions_passed: 0, assertions_failed: 0, duration_ms: 0, stopped: false };

const snap = (status: number) => ({ status, duration_ms: 10, size: 2, headers: [] as [string, string][], body: "{}", truncated: false });

function cmp(path: string, over: Partial<RequestComparison> = {}): RequestComparison {
  return { path, name: path.replace(".yml", ""), same: true, status: null, headers: [], body: [], body_text: false, tests: [], error: null, sides: { old: snap(200), new: snap(200) }, ...over };
}

function change(path: string, kind: ModelChange["change"], fields = 1): ModelChange {
  return {
    target: { kind: "request", path },
    change: kind,
    file: path,
    name: path.replace(".yml", ""),
    fields: Array.from({ length: fields }, (_, i) => ({ path: ["url", String(i)], old: { kind: "text", text: "a" }, new: { kind: "text", text: "b" } })),
  };
}

const run = (requests: RequestComparison[]): RunComparison => ({ from: commit("a"), to: commit("b"), requests, from_summary: summary, to_summary: summary });

describe("compare rows", () => {
  const diff: CollectionDiff = {
    changes: [
      change("edited.yml", { kind: "modified" }, 2),
      change("added.yml", { kind: "added" }),
      change("removed.yml", { kind: "removed" }),
      change("new/place.yml", { kind: "moved", from: "old/place.yml" }),
      { target: { kind: "folder", path: "api" }, change: { kind: "modified" }, file: "api/folder.yml", name: "api", fields: [] },
    ],
    files: [],
  };
  const requests = [
    cmp("same.yml"),
    cmp("edited.yml", { same: false, status: { old: 200, new: 500 }, sides: { old: snap(200), new: snap(500) }, tests: [{ name: "status is 2xx", old: true, new: false }] }),
    cmp("answer.yml", { same: false, body: [{ pointer: "/id", old: 1, new: 2 }] }),
    cmp("added.yml", { same: false, status: { old: null, new: 200 }, sides: { old: null, new: snap(200) } }),
    cmp("removed.yml", { same: false, status: { old: 200, new: null }, sides: { old: snap(200), new: null } }),
    cmp("old/place.yml", { same: false, status: { old: 200, new: null }, sides: { old: snap(201), new: null }, tests: [{ name: "t", old: false, new: null }] }),
    cmp("new/place.yml", { same: false, status: { old: null, new: 200 }, sides: { old: null, new: snap(200) }, tests: [{ name: "t", old: null, new: true }] }),
    cmp("broken.yml", { same: false, error: { old: null, new: "connection failed" }, sides: { old: snap(200), new: null }, status: { old: 200, new: null } }),
  ];
  const contract: SpecChange[] = [
    { kind: "operation-removed", breaking: true, operation: "GET /pets", location: "", message: "removed", requests: ["same.yml"] },
    { kind: "operation-added", breaking: false, operation: "GET /cats", location: "", message: "added", requests: ["answer.yml"] },
  ] as SpecChange[];
  const rows = buildRows(diff, run(requests), contract);
  const by = (p: string) => rows.find((r) => r.path === p)!;

  it("joins definition, answers and contract per request, in run order", () => {
    expect(rows.map((r) => r.path)).toEqual(["same.yml", "edited.yml", "answer.yml", "added.yml", "removed.yml", "new/place.yml", "broken.yml"]);
    expect(by("same.yml").state).toBe("same");
    expect(by("same.yml").breaking).toHaveLength(1);
    expect(by("answer.yml").breaking).toHaveLength(0);
    expect(by("edited.yml").state).toBe("changed");
    expect(by("edited.yml").regressed).toEqual(["status is 2xx"]);
    expect(by("answer.yml").state).toBe("changed");
    expect(by("added.yml").state).toBe("new");
    expect(by("removed.yml").state).toBe("removed");
    expect(by("broken.yml").failing).toBe(true);
  });

  it("shows a moved request once, with both answers", () => {
    const moved = by("new/place.yml");
    expect(moved.oldPath).toBe("old/place.yml");
    expect(moved.state).toBe("changed");
    expect(moved.response?.sides?.old?.status).toBe(201);
    expect(moved.response?.sides?.new?.status).toBe(200);
    expect(moved.response?.status).toEqual({ old: 201, new: 200 });
    expect(moved.response?.tests).toEqual([{ name: "t", old: false, new: true }]);
    expect(rows.filter((r) => r.path === "old/place.yml")).toHaveLength(0);
  });

  it("counts the header: changed, new, removed, breaking, green to red", () => {
    expect(headerCounts(rows)).toEqual({ total: 7, changed: 4, added: 1, removed: 1, same: 1, breaking: 1, regressed: 1, fixed: 1 });
  });

  it("works with the definitions only and with the answers only", () => {
    const definitions = buildRows(diff, null);
    expect(definitions.map((r) => [r.path, r.state])).toEqual([
      ["edited.yml", "changed"],
      ["added.yml", "new"],
      ["removed.yml", "removed"],
      ["new/place.yml", "changed"],
    ]);
    const answers = buildRows(null, run(requests.slice(0, 5)));
    expect(answers.map((r) => r.state)).toEqual(["same", "changed", "changed", "new", "removed"]);
  });

  it("filters by changes, failures and text", () => {
    expect(filterRows(rows, "all")).toHaveLength(7);
    // A breaking contract change counts as a change even when the answers are the same.
    expect(filterRows(rows, "changes").map((r) => r.path)).toContain("same.yml");
    expect(filterRows(buildRows(diff, run(requests)), "changes")).toHaveLength(6);
    expect(filterRows(buildRows(diff, run([cmp("calm.yml")])), "changes").map((r) => r.path)).not.toContain("calm.yml");
    expect(filterRows(rows, "failures").map((r) => r.path)).toEqual(["edited.yml", "broken.yml"]);
    expect(filterRows(rows, "new").map((r) => r.path)).toEqual(["added.yml"]);
    expect(filterRows(rows, "removed").map((r) => r.path)).toEqual(["removed.yml"]);
    expect(filterRows(rows, "changed")).toHaveLength(4);
    expect(filterRows(rows, "breaking").map((r) => r.path)).toEqual(["same.yml"]);
    expect(filterRows(rows, "all", "PLACE").map((r) => r.path)).toEqual(["new/place.yml"]);
    expect(filterRows(rows, "all", "old/place").map((r) => r.path)).toEqual(["new/place.yml"]);
  });

  it("steps from change to change with n and p, wrapping around", () => {
    const list = buildRows(null, run([cmp("a.yml"), cmp("b.yml", { same: false, body_text: true }), cmp("c.yml"), cmp("d.yml", { same: false, body_text: true })]));
    expect(nextChange(list, -1, 1)).toBe(1);
    expect(nextChange(list, 1, 1)).toBe(3);
    expect(nextChange(list, 3, 1)).toBe(1);
    expect(nextChange(list, 1, -1)).toBe(3);
    expect(nextChange(list, -1, -1)).toBe(3);
    expect(nextChange(buildRows(null, run([cmp("a.yml")])), -1, 1)).toBe(-1);
    expect(nextChange([], -1, 1)).toBe(-1);
  });

  it("words each row", () => {
    expect(rowSummary(by("same.yml"))).toBe("breaking API change");
    expect(rowSummary(by("edited.yml"))).toBe("2 fields edited · status 200 → 500 · 1 test now failing");
    expect(rowSummary(by("answer.yml"))).toBe("answer differs");
    expect(rowSummary(by("added.yml"))).toBe("new at the newer revision");
    expect(rowSummary(by("new/place.yml"))).toContain("moved from old/place.yml");
    expect(rowSummary(by("broken.yml"))).toContain("fails to send");
  });
});
