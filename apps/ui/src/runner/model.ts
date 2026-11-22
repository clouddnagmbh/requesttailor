// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The runner's view model, without Svelte (unit-tested): the steps of a run as the events
// arrive, live counts, the window of rows to render, and the data preview. Owner: wave 7,
// strand D.

import type { DataRow, RunEvent, RunPlan, RunStep, RunSummary } from "../commands/types";

/** Counts the summary also has; kept live while the run goes on. */
export interface Counts {
  requests: number;
  passed: number;
  failed: number;
  skipped: number;
  tests_passed: number;
  tests_failed: number;
  assertions_passed: number;
  assertions_failed: number;
}

export const emptyCounts = (): Counts => ({
  requests: 0,
  passed: 0,
  failed: 0,
  skipped: 0,
  tests_passed: 0,
  tests_failed: 0,
  assertions_passed: 0,
  assertions_failed: 0,
});

/** Passed and failed tests and assertions of one step. */
export function stepCounts(step: RunStep): { tests: [number, number]; assertions: [number, number] } {
  const tests = step.report.tests ?? [];
  const assertions = step.report.assertions ?? [];
  const passed = <T extends { passed: boolean }>(list: T[]) => list.filter((t) => t.passed).length;
  return {
    tests: [passed(tests), tests.length - passed(tests)],
    assertions: [passed(assertions), assertions.length - passed(assertions)],
  };
}

export type StepState = "passed" | "failed" | "skipped";

export function stepState(step: RunStep): StepState {
  if (step.skipped) return "skipped";
  return step.passed ? "passed" : "failed";
}

/** A run's results, mutated in place by `apply` (the Svelte side re-renders on a counter). */
export class RunResults {
  steps: RunStep[] = [];
  total = 0;
  /** The request being sent, `null` between requests and after the run. */
  current: { iteration: number; request_path: string } | null = null;
  counts: Counts = emptyCounts();
  summary: RunSummary | null = null;

  apply(event: RunEvent): void {
    switch (event.kind) {
      case "started":
        this.total = event.total;
        break;
      case "request":
        this.current = { iteration: event.iteration, request_path: event.request_path };
        break;
      case "step": {
        const step = event.step;
        this.steps.push(step);
        this.current = null;
        const c = this.counts;
        c.requests++;
        const state = stepState(step);
        if (state === "skipped") c.skipped++;
        else if (state === "passed") c.passed++;
        else c.failed++;
        const n = stepCounts(step);
        c.tests_passed += n.tests[0];
        c.tests_failed += n.tests[1];
        c.assertions_passed += n.assertions[0];
        c.assertions_failed += n.assertions[1];
        break;
      }
      case "finished":
        this.summary = event.summary;
        this.current = null;
        break;
    }
  }
}

/** The rows to render of a list with fixed row heights: `[start, end)`. */
export function windowRange(
  scrollTop: number,
  viewport: number,
  rowHeight: number,
  count: number,
  overscan = 8,
): [number, number] {
  if (count === 0 || rowHeight <= 0) return [0, 0];
  const first = Math.floor(Math.max(0, scrollTop) / rowHeight);
  const visible = Math.ceil(Math.max(0, viewport) / rowHeight) + 1;
  const start = Math.max(0, Math.min(count, first - overscan));
  const end = Math.min(count, first + visible + overscan);
  return [start, Math.max(start, end)];
}

/** The scroll position that keeps row `index` in view (unchanged if it is). */
export function scrollToShow(index: number, scrollTop: number, viewport: number, rowHeight: number): number {
  const top = index * rowHeight;
  if (top < scrollTop) return top;
  if (top + rowHeight > scrollTop + viewport) return top + rowHeight - viewport;
  return scrollTop;
}

/** Columns (in first-seen order) and the first `max` rows as text, for the data preview. */
export function dataPreview(rows: DataRow[], max = 5): { columns: string[]; rows: string[][] } {
  const columns: string[] = [];
  for (const row of rows) for (const [name] of row) if (!columns.includes(name)) columns.push(name);
  const text = (v: unknown) => (typeof v === "string" ? v : v === undefined ? "" : JSON.stringify(v));
  return {
    columns,
    rows: rows.slice(0, max).map((row) => columns.map((c) => text(row.find(([n]) => n === c)?.[1]))),
  };
}

export interface RunConfig {
  iterations: number;
  delayMs: number;
  bail: boolean;
  data: DataRow[] | null;
}

/** The plan for `requests` with `config` (iterations at least 1; ignored with data). */
export function planOf(requests: string[], config: RunConfig): RunPlan {
  const whole = (n: number, min: number) => (Number.isFinite(n) ? Math.max(min, Math.floor(n)) : min);
  return {
    requests,
    data: config.data ?? [],
    iterations: whole(config.iterations, 1),
    delay_ms: whole(config.delayMs, 0),
    bail: config.bail,
  };
}

/** "3 of 12" style progress; `total` may be exceeded by `setNext` jumps. */
export function progressText(done: number, total: number): string {
  return `${done} of ${Math.max(done, total)}`;
}

/** A step's time in ms (the response's total), or `null` without a response. */
export function stepTime(step: RunStep): number | null {
  return step.report.response?.total ?? null;
}
