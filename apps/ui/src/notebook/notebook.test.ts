// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { mockNotebook } from "../commands/mock-notebook";
import type { SendReport } from "../commands/types";
import { MASK, bodyText, carriedVars, chartPoints, newPath, statusLine } from "./model";

const host = {
  getRequest: async () => {
    throw new Error("no");
  },
  policy: async () => ({ environments: [{ environment: "PRD", production: true }] }) as never,
  environments: async () => [{ name: "Test" }, { name: "PRD" }],
};

describe("notebook model", () => {
  it("carries variables, but never a masked value", () => {
    expect(
      carriedVars([
        { block: "a", set: [["token", MASK], ["id", "1"]] },
        { block: "b", set: [["id", "2"]] },
      ]),
    ).toEqual([["id", "2"]]);
  });

  it("names new files without clashes", () => {
    expect(newPath("My Orders!", [])).toBe("my-orders.notebook.md");
    expect(newPath("a", ["a.notebook.md", "a-2.notebook.md"])).toBe("a-3.notebook.md");
    expect(newPath("  ", [])).toBe("notebook.notebook.md");
  });

  it("makes chart points inside the box and skips non-numbers", () => {
    const pts = chartPoints([1, "x", 3], 100, 50);
    expect(pts).toHaveLength(2);
    expect(pts.every((p) => p.x >= 0 && p.x <= 100 && p.y >= 0 && p.y <= 50)).toBe(true);
    expect(chartPoints([], 10, 10)).toEqual([]);
  });

  it("shows the guard's refusal and pretty JSON", () => {
    const refused = { response: null, error: { kind: "blocked", message: { rule: "r", reason: "no" } } } as unknown as SendReport;
    expect(statusLine(refused)).toContain("refused by the send guard (r)");
    const ok = { response: { body: { encoding: "utf8", data: '{"a":1}' } } } as unknown as SendReport;
    expect(bodyText(ok).text).toBe('{\n  "a": 1\n}');
  });
});

describe("mock notebook", () => {
  it("runs blocks in order, passes variables on and masks credentials", async () => {
    const api = mockNotebook(host);
    const [entry] = await api.list();
    const out = await api.run({ path: entry.path, environment: "Test" });
    expect(out.results.map((r) => r.block)).toEqual(["login", "list", "create"]);
    expect(out.results[0].set).toEqual([["token", MASK], ["userId", "42"]]);
    expect(out.results[1].views?.[0].result.rows).toHaveLength(3);
    expect(out.results[2].skipped).toBe("skipped (writes)");
  });

  it("never writes to production and allows writes elsewhere", async () => {
    const api = mockNotebook(host);
    const path = (await api.list())[0].path;
    const prd = await api.run({ path, environment: "PRD", allow_writes: true, block: "create" });
    expect(prd.results[0].skipped).toBe("skipped (production)");
    const test = await api.run({ path, environment: "Test", allow_writes: true, block: "create" });
    expect(test.results[0].report).toBeDefined();
  });
});
