// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the compare and bisect views (wave 12, strand C).

import type { BisectOutcome, BisectStep, BisectVerdict, RequestComparison, RunComparison } from "../commands/types";

/** The first seven characters of a commit id. */
export function shortOid(oid: string): string {
  return oid.slice(0, 7);
}

export interface ComparisonCounts {
  total: number;
  changed: number;
  same: number;
}

export function comparisonCounts(cmp: RunComparison): ComparisonCounts {
  const changed = cmp.requests.filter((r) => !r.same).length;
  return { total: cmp.requests.length, changed, same: cmp.requests.length - changed };
}

/** `200 → 404`; a side without a value is `none`. */
export function arrow<T>(old: T | null, now: T | null): string {
  const show = (v: T | null) => (v === null ? "none" : typeof v === "string" ? v : JSON.stringify(v));
  return `${show(old)} → ${show(now)}`;
}

/** What differs in one request, in a few words (`status 200 → 404, 2 headers, 3 body values`). */
export function changeSummary(r: RequestComparison): string {
  if (r.same) return "unchanged";
  const parts: string[] = [];
  if (r.status) parts.push(`status ${arrow(r.status.old, r.status.new)}`);
  if (r.error) parts.push("error");
  if (r.headers.length > 0) parts.push(`${r.headers.length} ${r.headers.length === 1 ? "header" : "headers"}`);
  if (r.body.length > 0) parts.push(`${r.body.length} body ${r.body.length === 1 ? "value" : "values"}`);
  if (r.body_text) parts.push("body text");
  if (r.tests.length > 0) parts.push(`${r.tests.length} ${r.tests.length === 1 ? "test" : "tests"}`);
  return parts.length > 0 ? parts.join(", ") : "not run on one side";
}

/** A JSON value for one line, cut to `max` characters. */
export function valueText(v: unknown | null, max = 80): string {
  if (v === null || v === undefined) return "none";
  const text = typeof v === "string" ? JSON.stringify(v) : JSON.stringify(v);
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

export const VERDICT_LABEL: Record<BisectVerdict, string> = { good: "good", bad: "bad", skip: "skipped" };

/** `3 of 49 commits tested`. */
export function bisectProgress(steps: BisectStep[], candidates: number): string {
  return `${steps.length} of ${candidates} commits tested`;
}

/** The result line of a finished bisect. */
export function bisectResult(outcome: BisectOutcome): string {
  if (outcome.first_bad) return `First bad commit: ${shortOid(outcome.first_bad.oid)} ${outcome.first_bad.summary}`;
  if (outcome.steps[0]?.verdict === "good") return "The bad revision passes: nothing fails.";
  if (outcome.steps.length === 0) return "No commits between good and bad.";
  return "No first failing commit could be told: only skipped commits are left, or the newest cannot be tested.";
}
