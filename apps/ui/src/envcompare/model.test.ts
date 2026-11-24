// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { AgentPolicy, EnvComparison, EnvPair, RequestComparison } from "../commands/types";
import { canCompare, defaultSelection, makeReference, pairCounts, pairTitle, productionNames, rowsOfPair, toggle } from "./model";

const summary = { requests: 0, passed: 0, failed: 0, skipped: 0, tests_passed: 0, tests_failed: 0, assertions_passed: 0, assertions_failed: 0, duration_ms: 0, stopped: false };
const snap = (status: number, body = "{}") => ({ status, duration_ms: 5, size: 2, headers: [] as [string, string][], body, truncated: false });

function cmp(path: string, over: Partial<RequestComparison> = {}): RequestComparison {
  return { path, name: path.replace(".yml", ""), same: true, status: null, headers: [], body: [], body_text: false, tests: [], error: null, sides: { old: snap(200), new: snap(200) }, ...over };
}

const pair: EnvPair = {
  environment: "qa",
  requests: [
    cmp("a.yml"),
    cmp("b.yml", { same: false, status: { old: 200, new: 404 } }),
    cmp("c.yml", { same: false, body: [{ pointer: "/n", old: 1, new: 2 }] }),
    cmp("d.yml", { same: false, error: { old: null, new: "refused" }, sides: { old: snap(200), new: null } }),
  ],
  reference_summary: summary,
  summary,
};
const result: EnvComparison = { reference: "dev", pairs: [pair], skipped: [] };

describe("environment selection", () => {
  it("keeps the order of choosing and the first is the reference", () => {
    expect(toggle(toggle([], "dev"), "qa")).toEqual(["dev", "qa"]);
    expect(toggle(["dev", "qa"], "dev")).toEqual(["qa"]);
    expect(makeReference(["dev", "qa", "uat"], "uat")).toEqual(["uat", "dev", "qa"]);
    expect(makeReference(["dev"], "nowhere")).toEqual(["dev"]);
  });
  it("needs two environments", () => {
    expect(canCompare(["dev"])).toBe(false);
    expect(canCompare(["dev", "qa"])).toBe(true);
  });
  it("starts with the active environment and another one", () => {
    expect(defaultSelection(["dev", "qa", "prod"], "qa")).toEqual(["qa", "dev"]);
    expect(defaultSelection(["dev", "qa"], null)).toEqual(["dev", "qa"]);
    expect(defaultSelection(["dev"], "dev")).toEqual([]);
  });
  it("finds production by the policy flag, else the name", () => {
    const policy: AgentPolicy = { access: "read", environments: [{ environment: "qa", access: "read", production: true }, { environment: "prd-clone", access: "read", production: false }] };
    expect(productionNames(policy, ["dev", "qa", "prd-clone", "live"])).toEqual(["qa", "live"]);
    expect(productionNames(null, ["dev", "PRD"])).toEqual(["PRD"]);
  });
});

describe("a pair", () => {
  it("counts what differs", () => {
    expect(pairCounts(pair)).toEqual({ total: 4, different: 3, same: 1, status: 1, errors: 1 });
    expect(pairTitle(result, pair)).toBe("dev (reference) against qa: 3 of 4 requests differ");
  });
  it("makes the rows of the revision comparison", () => {
    const rows = rowsOfPair(pair);
    expect(rows.map((r) => [r.path, r.state])).toEqual([
      ["a.yml", "same"],
      ["b.yml", "changed"],
      ["c.yml", "changed"],
      ["d.yml", "changed"],
    ]);
    expect(rows[1]!.response?.status).toEqual({ old: 200, new: 404 });
  });
});
