// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { SendReport } from "../commands/types";
import { scriptNotes, testsLabel, testSummary } from "./tests";

const base: SendReport = {
  version: 1,
  request_path: "a.yml",
  environment: null,
  request: null,
  response: null,
  variables: [],
  unresolved: [],
  error: null,
};

describe("test results", () => {
  it("counts tests and assertions, failed first", () => {
    const summary = testSummary({
      ...base,
      tests: [
        { name: "status is 200", passed: true, origin: "request" },
        { name: "has id", passed: false, error: "expected undefined to be 7", origin: "folder:users" },
      ],
      assertions: [
        { expression: "res.status", operator: "eq", expected: "200", actual: "200", passed: true },
        { expression: "res.body.id", operator: "eq", expected: "7", actual: "8", passed: false },
        { expression: "res.body", operator: "isJson", expected: "", actual: "{}", passed: true },
        { expression: "res.x", operator: "eq", expected: "1", actual: null, passed: false, error: "`res.x` is not known" },
      ],
    });
    expect(summary.passed).toBe(3);
    expect(summary.total).toBe(6);
    expect(testsLabel(summary)).toBe("Tests 3/6");
    expect(summary.rows.map((r) => [r.passed, r.title])).toEqual([
      [false, "has id"],
      [false, "res.body.id eq 7"],
      [false, "res.x eq 1"],
      [true, "status is 200"],
      [true, "res.status eq 200"],
      [true, "res.body isJson"],
    ]);
    expect(summary.rows[0].detail).toBe("expected undefined to be 7");
    expect(summary.rows[0].origin).toBe("folder:users");
    expect(summary.rows[1].detail).toBe("expected 7, got 8");
    expect(summary.rows[2].detail).toBe("`res.x` is not known");
  });

  it("has a plain label without results", () => {
    expect(testsLabel(testSummary(base))).toBe("Tests");
  });

  it("picks the script notes", () => {
    const notes = scriptNotes({
      ...base,
      notes: [
        { source: "auth", level: "info", text: "token fetched" },
        { source: "script", level: "warning", text: "Scripts were not run: this collection is not trusted yet." },
        { source: "script:tests", level: "error", text: "ReferenceError: x is not defined" },
      ],
    });
    expect(notes.map((n) => n.source)).toEqual(["script", "script:tests"]);
  });
});
