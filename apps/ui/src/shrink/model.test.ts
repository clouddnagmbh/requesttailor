// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { ShrinkResult } from "../commands/types";
import { emptyRequest } from "../commands/mock";
import {
  buildPredicate,
  choiceOf,
  describePredicate,
  failed,
  headline,
  notes,
  outcomeOf,
  parseKeep,
  partName,
  removedGroups,
  stoppedText,
  writeQuestion,
} from "./model";

const result = (over: Partial<ShrinkResult> = {}): ShrinkResult => ({
  request: emptyRequest("r"),
  removed: [
    { part: "query:a", kind: "query" },
    { part: "header:X-Debug", kind: "header" },
    { part: "body:$.items[0]", kind: "body" },
    { part: "header:X-Trace", kind: "header" },
  ],
  probes: 14,
  one_minimal: true,
  ...over,
});

describe("buildPredicate", () => {
  it("builds each kind and says what is missing", () => {
    expect(buildPredicate("last", "")).toEqual({ kind: "last-failure" });
    expect(buildPredicate("status", " 500 ")).toEqual({ kind: "status", status: 500 });
    expect(buildPredicate("status", "abc")).toMatch(/between 100 and 599/);
    expect(buildPredicate("status", "99")).toMatch(/between/);
    expect(buildPredicate("server-error", "")).toEqual({ kind: "server-error" });
    expect(buildPredicate("assertion", "")).toEqual({ kind: "assertion-fails" });
    expect(buildPredicate("body", "boom-\\d+")).toEqual({ kind: "body-matches", pattern: "boom-\\d+" });
    expect(buildPredicate("body", "(")).toMatch(/not valid/);
    expect(buildPredicate("body", "")).toMatch(/Enter/);
    expect(buildPredicate("header-present", "X-Id")).toEqual({ kind: "header-present", name: "X-Id" });
    expect(buildPredicate("header-missing", "")).toMatch(/Enter/);
  });

  it("round-trips through choiceOf", () => {
    for (const p of [
      { kind: "status", status: 502 },
      { kind: "server-error" },
      { kind: "assertion-fails" },
      { kind: "body-matches", pattern: "x" },
      { kind: "header-present", name: "A" },
      { kind: "header-missing", name: "B" },
      { kind: "last-failure" },
    ] as const) {
      const { choice, value } = choiceOf(p);
      expect(buildPredicate(choice, value)).toEqual(p);
    }
  });

  it("describes a predicate in words", () => {
    expect(describePredicate({ kind: "status", status: 500 })).toBe("status is 500");
    expect(describePredicate({ kind: "assertion-fails", assertion: "0" })).toBe("assertion 0 fails");
  });
});

describe("keep and failure", () => {
  it("splits keep by comma and line", () => {
    expect(parseKeep("header:Authorization, query:key\n body:$.id ,")).toEqual(["header:Authorization", "query:key", "body:$.id"]);
    expect(parseKeep("  ")).toEqual([]);
  });

  it("knows a failed response", () => {
    const stored = (status: number, assertions: { passed: boolean }[] = []) =>
      ({ report: { response: { status }, assertions, tests: [] } }) as never;
    expect(failed(undefined)).toBe(false);
    expect(failed(stored(200))).toBe(false);
    expect(failed(stored(404))).toBe(true);
    expect(failed(stored(200, [{ passed: false }]))).toBe(true);
  });
});

describe("results", () => {
  it("counts parts and values", () => {
    expect(headline(result())).toBe("Removed 4 parts in 14 probes.");
    expect(headline(result({ removed: [{ part: "header:A", kind: "header" }], probes: 1 }))).toBe("Removed 1 part in 1 probe.");
    expect(headline(result({ removed: [{ part: "header:A", kind: "header" }, { part: "body:$.n", kind: "value" }] }))).toBe(
      "Removed 1 part in 14 probes, simplified 1 value.",
    );
  });

  it("tells not reproduced and flaky", () => {
    expect(outcomeOf(result({ not_reproduced: true, removed: [] }))).toBe("not-reproduced");
    expect(headline(result({ not_reproduced: true, removed: [] }))).toMatch(/did not show again/);
    expect(outcomeOf(result({ flaky: true, removed: [] }))).toBe("flaky");
    expect(outcomeOf(result({ flaky: true }))).toBe("found");
    expect(outcomeOf(result({ stopped_by: "probes" }))).toBe("stopped");
  });

  it("explains why it stopped", () => {
    expect(stoppedText("probes")).toMatch(/limit of probes/);
    expect(stoppedText("blocked: no-shrink")).toMatch(/no-shrink/);
    expect(notes(result({ stopped_by: "time", one_minimal: false }))[0]).toMatch(/time limit/);
    expect(notes(result({ flaky: true, one_minimal: false }))[0]).toMatch(/Flaky/);
    expect(notes(result())[0]).toMatch(/1-minimal/);
  });

  it("groups what was removed by kind and names parts without values", () => {
    const groups = removedGroups(result());
    expect(groups.map((g) => g.title)).toEqual(["Headers", "Query parameters", "Body"]);
    expect(groups[0]!.steps.map(partName)).toEqual(["X-Debug", "X-Trace"]);
    expect(partName({ part: "body:$.items[0]", kind: "body" })).toBe("$.items[0]");
  });

  it("words the question for writing requests", () => {
    expect(writeQuestion(100)).toBe("This sends up to 100 requests that write.");
  });
});
