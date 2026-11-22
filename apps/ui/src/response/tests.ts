// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Test and assertion results of a send, for the response's "Tests" tab (wave 7, strand C).

import type { Note, SendReport } from "../commands/types";

export interface ResultRow {
  kind: "test" | "assertion";
  passed: boolean;
  /** Test name, or `expression operator value`. */
  title: string;
  /** Failure message (assertion message, evaluation error, expected/actual). */
  detail: string | null;
  /** Where a test comes from (`collection`, `folder:<path>`, `request`). */
  origin: string | null;
}

export interface TestSummary {
  passed: number;
  total: number;
  /** Failed first, then passed; otherwise in report order. */
  rows: ResultRow[];
}

export function testSummary(report: SendReport): TestSummary {
  const rows: ResultRow[] = [
    ...(report.tests ?? []).map(
      (t): ResultRow => ({ kind: "test", passed: t.passed, title: t.name, detail: t.error ?? null, origin: t.origin }),
    ),
    ...(report.assertions ?? []).map((a): ResultRow => {
      const title = [a.expression, a.operator, a.expected].filter((p) => p !== "").join(" ");
      const detail = a.error ?? (a.passed ? null : `expected ${a.expected || "(empty)"}, got ${a.actual ?? "undefined"}`);
      return { kind: "assertion", passed: a.passed, title, detail, origin: null };
    }),
  ];
  const passed = rows.filter((r) => r.passed).length;
  return { passed, total: rows.length, rows: [...rows.filter((r) => !r.passed), ...rows.filter((r) => r.passed)] };
}

/** The tab label: `Tests`, or `Tests 3/4`. */
export function testsLabel(summary: TestSummary): string {
  return summary.total === 0 ? "Tests" : `Tests ${summary.passed}/${summary.total}`;
}

/** Notes about scripts (not trusted, failed, skipped), shown with the results. */
export function scriptNotes(report: SendReport): Note[] {
  return (report.notes ?? []).filter((n) => n.source === "script" || n.source.startsWith("script:"));
}
