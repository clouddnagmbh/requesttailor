// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { RunStep, SendReport } from "../commands/types";
import { dataPreview, planOf, progressText, RunResults, scrollToShow, stepCounts, stepState, windowRange } from "./model";

function report(extra: Partial<SendReport> = {}): SendReport {
  return {
    version: 1,
    request_path: "a.yml",
    environment: null,
    request: null,
    response: null,
    variables: [],
    unresolved: [],
    error: null,
    ...extra,
  };
}

function step(passed: boolean, extra: Partial<RunStep> = {}, r: Partial<SendReport> = {}): RunStep {
  return {
    iteration: 0,
    request_path: "a.yml",
    request_name: "A",
    report: report(r),
    passed,
    skipped: false,
    ...extra,
  };
}

describe("runner model", () => {
  it("applies events and keeps live counts", () => {
    const r = new RunResults();
    r.apply({ kind: "started", total: 3 });
    expect(r.total).toBe(3);
    r.apply({ kind: "request", iteration: 0, request_path: "a.yml" });
    expect(r.current).toEqual({ iteration: 0, request_path: "a.yml" });
    r.apply({
      kind: "step",
      step: step(true, {}, { tests: [{ name: "t", passed: true, origin: "request" }] }),
    });
    expect(r.current).toBeNull();
    r.apply({
      kind: "step",
      step: step(
        false,
        {},
        {
          tests: [
            { name: "t", passed: true, origin: "request" },
            { name: "u", passed: false, error: "no", origin: "request" },
          ],
          assertions: [{ expression: "res.status", operator: "eq", expected: "200", actual: "500", passed: false }],
        },
      ),
    });
    r.apply({ kind: "step", step: step(true, { skipped: true }) });
    expect(r.steps).toHaveLength(3);
    expect(r.counts).toEqual({
      requests: 3,
      passed: 1,
      failed: 1,
      skipped: 1,
      tests_passed: 2,
      tests_failed: 1,
      assertions_passed: 0,
      assertions_failed: 1,
    });
    const summary = { ...r.counts, duration_ms: 5, stopped: false };
    r.apply({ kind: "finished", summary });
    expect(r.summary).toEqual(summary);
  });

  it("names a step's state and counts its checks", () => {
    expect(stepState(step(true))).toBe("passed");
    expect(stepState(step(false))).toBe("failed");
    expect(stepState(step(true, { skipped: true }))).toBe("skipped");
    const s = step(false, {}, { tests: [{ name: "u", passed: false, origin: "request" }] });
    expect(stepCounts(s)).toEqual({ tests: [0, 1], assertions: [0, 0] });
  });

  it("windows a long list", () => {
    // 10 000 rows of 28 px, a 280 px viewport: never more than the visible rows plus overscan.
    expect(windowRange(0, 280, 28, 10_000, 8)).toEqual([0, 19]);
    const [start, end] = windowRange(28 * 5000, 280, 28, 10_000, 8);
    expect(start).toBe(4992);
    expect(end).toBe(5019);
    expect(windowRange(28 * 9999, 280, 28, 10_000, 8)).toEqual([9991, 10_000]);
    expect(windowRange(0, 280, 28, 0)).toEqual([0, 0]);
    expect(windowRange(-50, 280, 28, 3)).toEqual([0, 3]);
  });

  it("scrolls a row into view only when needed", () => {
    expect(scrollToShow(0, 100, 280, 28)).toBe(0);
    expect(scrollToShow(5, 0, 280, 28)).toBe(0);
    expect(scrollToShow(20, 0, 280, 28)).toBe(21 * 28 - 280);
  });

  it("previews data rows with all columns", () => {
    const p = dataPreview(
      [
        [
          ["name", "ada"],
          ["id", 1],
        ],
        [
          ["name", "bob"],
          ["tags", ["x"]],
        ],
        [["name", "eve"]],
      ],
      2,
    );
    expect(p.columns).toEqual(["name", "id", "tags"]);
    expect(p.rows).toEqual([
      ["ada", "1", ""],
      ["bob", "", '["x"]'],
    ]);
  });

  it("builds the plan from the configuration", () => {
    expect(planOf(["a.yml"], { iterations: 2.7, delayMs: -5, bail: true, data: null })).toEqual({
      requests: ["a.yml"],
      data: [],
      iterations: 2,
      delay_ms: 0,
      bail: true,
    });
    expect(planOf([], { iterations: Number.NaN, delayMs: 100, bail: false, data: [[["a", "1"]]] }).iterations).toBe(1);
    expect(progressText(3, 10)).toBe("3 of 10");
    expect(progressText(12, 10)).toBe("12 of 12");
  });
});
