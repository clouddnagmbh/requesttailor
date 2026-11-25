// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { ResponseView, ViewResult } from "../commands/types";
import { CELL_LIMIT, cellText, jsonOf, looksJson, nameProblem, replaces, splitColumns, textLines, viewLabel } from "./model";

const view = (name: string, language: ResponseView["language"] = "jsonpath"): ResponseView => ({ name, language, expression: "$", display: "json" });
const result = (values: unknown[]): ViewResult => ({ display: "json", values });

describe("names", () => {
  it("need 1 to 80 characters without control characters", () => {
    expect(nameProblem("Ids")).toBeNull();
    expect(nameProblem("   ")).not.toBeNull();
    expect(nameProblem("x".repeat(81))).toMatch(/80/);
    expect(nameProblem("a\nb")).not.toBeNull();
  });
  it("replace a view of the same name", () => {
    expect(replaces(" Ids ", [view("Ids")])).toBe(true);
    expect(replaces("Other", [view("Ids")])).toBe(false);
  });
});

describe("columns", () => {
  it("are split at commas, trimmed, without empties or duplicates", () => {
    expect(splitColumns(" id, name ,,id,email")).toEqual(["id", "name", "email"]);
    expect(splitColumns("")).toEqual([]);
  });
});

describe("cells", () => {
  it("read as text, JSON or empty and are cut", () => {
    expect(cellText("Ada")).toBe("Ada");
    expect(cellText(3)).toBe("3");
    expect(cellText(null)).toBe("");
    expect(cellText({ a: 1 })).toBe('{"a":1}');
    expect(cellText("x".repeat(CELL_LIMIT + 5))).toHaveLength(CELL_LIMIT + 1);
  });
});

describe("results", () => {
  it("show one match as itself and several as a list", () => {
    expect(jsonOf(result([1]))).toBe(1);
    expect(jsonOf(result([1, 2]))).toEqual([1, 2]);
    expect(jsonOf(result([]))).toEqual([]);
  });
  it("give text lines", () => {
    expect(textLines(result(["a", 2, { b: 1 }]))).toEqual(["a", "2", '{"b":1}']);
  });
  it("label a jq view", () => {
    expect(viewLabel(view("A"))).toBe("A");
    expect(viewLabel(view("A", "jq"))).toBe("A (jq)");
  });
});

describe("looksJson", () => {
  it("goes by the content type, then by the first character", () => {
    expect(looksJson("application/problem+json", "")).toBe(true);
    expect(looksJson("text/plain", ' {"a":1}')).toBe(true);
    expect(looksJson(null, "[1]")).toBe(true);
    expect(looksJson("text/html", "<p>")).toBe(false);
  });
});
