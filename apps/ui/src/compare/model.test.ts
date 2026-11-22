// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { BisectOutcome, GitCommit, RequestComparison } from "../commands/types";
import { arrow, bisectResult, changeSummary, shortOid, valueText } from "./model";

const commit: GitCommit = { oid: "0123456789abcdef", summary: "Move health", author: "A", time: 0 };
const same: RequestComparison = {
  path: "a.yml",
  name: "A",
  same: true,
  status: null,
  headers: [],
  body: [],
  body_text: false,
  tests: [],
  error: null,
};

describe("compare model", () => {
  it("shortens ids and shows changes", () => {
    expect(shortOid(commit.oid)).toBe("0123456");
    expect(arrow(200, 404)).toBe("200 → 404");
    expect(arrow<number>(null, 200)).toBe("none → 200");
    expect(valueText("x".repeat(100), 10)).toHaveLength(10);
    expect(valueText(null)).toBe("none");
  });

  it("summarises what differs", () => {
    expect(changeSummary(same)).toBe("unchanged");
    expect(
      changeSummary({
        ...same,
        same: false,
        status: { old: 200, new: 500 },
        headers: [{ name: "x", old: null, new: "1" }],
        body: [{ pointer: "/a", old: 1, new: 2 }],
        tests: [{ name: "t", old: true, new: false }],
      }),
    ).toBe("status 200 → 500, 1 header, 1 body value, 1 test");
  });

  it("words the result of a bisect", () => {
    const base: BisectOutcome = { first_bad: commit, candidates: 3, steps: [] };
    expect(bisectResult(base)).toBe("First bad commit: 0123456 Move health");
    expect(bisectResult({ ...base, first_bad: null, steps: [{ commit, verdict: "good", failed: [], remaining: 0 }] })).toMatch(/passes/);
    expect(bisectResult({ ...base, first_bad: null, candidates: 0 })).toMatch(/No commits/);
  });
});
