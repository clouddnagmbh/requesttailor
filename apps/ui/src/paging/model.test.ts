// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { PagingSpec } from "../commands/types";
import { draftOf, draftProblem, emptyDraft, limitsOf, originText, resultSummary, specOf, stopText } from "./model";

describe("paging rule draft", () => {
  it("round-trips every kind", () => {
    const specs: PagingSpec[] = [
      { rule: { kind: "link-header" } },
      { rule: { kind: "json-url", next: "$.next" }, merge: "$.items" },
      { rule: { kind: "cursor", next: "$.meta.cursor", param: { place: "header", name: "X-After" } }, merge: "$.data" },
    ];
    for (const spec of specs) expect(specOf(draftOf(spec))).toEqual(spec);
  });

  it("says what is missing", () => {
    expect(draftProblem(emptyDraft())).toBeNull();
    expect(draftProblem({ ...emptyDraft(), kind: "json-url", next: "next" })).toMatch(/starts with \$/);
    expect(draftProblem({ ...emptyDraft(), kind: "cursor", next: "$.c", name: " " })).toMatch(/Name the parameter/);
    expect(draftProblem({ ...emptyDraft(), merge: "items" })).toMatch(/merge path/);
  });

  it("never asks for more than the hard maximum", () => {
    expect(limitsOf(5000).max_pages).toBe(1000);
    expect(limitsOf(0).max_pages).toBe(20);
    expect(limitsOf(7).max_pages).toBe(7);
  });
});

describe("paging words", () => {
  it("names a plugin's rule", () => {
    expect(originText({ spec: { rule: { kind: "link-header" } }, origin: { kind: "plugin", plugin: "acme", id: "x" } })).toBe("Next page found (by acme)");
  });

  it("explains why a run stopped", () => {
    expect(stopText({ kind: "max-pages" })).toBe("page limit reached");
    expect(stopText({ kind: "error-status", message: 503 })).toBe("a page answered 503");
    expect(stopText({ kind: "refused", message: "rule: why" })).toContain("refused");
  });

  it("summarises a run", () => {
    const base = { response: {} as never, pages: 20, stopped: { kind: "max-pages" } as const, more: true };
    expect(resultSummary(base)).toBe("20 pages, stopped: page limit reached; more pages exist");
    expect(resultSummary({ ...base, pages: 1, stopped: { kind: "done" }, more: false })).toBe("1 page");
  });
});
