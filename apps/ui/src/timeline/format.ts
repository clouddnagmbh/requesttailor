// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Labels of the request history (wave 12, strand D).

import type { TestRecord } from "../commands/types";

export const shortOid = (oid: string): string => oid.slice(0, 7);

export function headline(count: number): string {
  return count === 1 ? "1 version" : `${count} versions`;
}

/** What the recorded result of a run says: `2 of 3 tests passed`. */
export function testSummary(r: TestRecord): string {
  const passed = r.tests.filter((t) => t.passed).length;
  const base = r.tests.length === 0 ? (r.passed ? "no tests, no error" : "request failed") : `${passed} of ${r.tests.length} tests passed`;
  return r.environment ? `${base} · ${r.environment}` : base;
}

/** Said when a restored version was put into the editor but not saved. */
export function restoreNote(label: string): string {
  return `Version ${label} is in the editor. Other requests have unsaved changes too: save when you are ready.`;
}
