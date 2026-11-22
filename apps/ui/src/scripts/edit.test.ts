// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { Script } from "../commands/types";
import {
  countScripts,
  expressionFor,
  isUnary,
  OPERATORS,
  scriptCode,
  valueHint,
  valueText,
  variableNameFor,
  withScriptCode,
} from "./edit";

describe("scripts", () => {
  const scripts: Script[] = [
    { phase: "tests", code: "test('a', () => {});" },
    { phase: "before-request", code: "rt.vars.set('x', 1);" },
    { phase: "tests", code: "// second tests script from an import" },
  ];

  it("edits the first script of a phase and keeps the others", () => {
    expect(scriptCode(scripts, "tests")).toBe("test('a', () => {});");
    expect(scriptCode(scripts, "after-response")).toBe("");
    const next = withScriptCode(scripts, "tests", "changed");
    expect(next.map((s) => s.code)).toEqual(["changed", "rt.vars.set('x', 1);", "// second tests script from an import"]);
    expect(scripts[0].code).toBe("test('a', () => {});");
  });

  it("adds a script for a new phase and removes one whose code is emptied", () => {
    const added = withScriptCode(scripts, "after-response", "rt.vars.set('y', 2);");
    expect(added.at(-1)).toEqual({ phase: "after-response", code: "rt.vars.set('y', 2);" });
    const removed = withScriptCode(scripts, "before-request", "  ");
    expect(removed.map((s) => s.phase)).toEqual(["tests", "tests"]);
    expect(withScriptCode([], "tests", "")).toEqual([]);
  });

  it("counts non-empty scripts", () => {
    expect(countScripts(scripts)).toBe(3);
    expect(countScripts([{ phase: "tests", code: " \n" }])).toBe(0);
  });
});

describe("assertion operators", () => {
  it("has Bruno's full set and knows the unary ones", () => {
    expect(OPERATORS).toHaveLength(28);
    expect(isUnary("isEmpty")).toBe(true);
    expect(isUnary("isArray")).toBe(true);
    expect(isUnary("eq")).toBe(false);
    expect(isUnary("between")).toBe(false);
    expect(valueHint("between")).toBe("low, high");
    expect(valueHint("isNull")).toBe("");
  });
});

describe("set as variable", () => {
  it("turns tree paths into expressions", () => {
    expect(expressionFor("$")).toBe("res.body");
    expect(expressionFor("$.token")).toBe("res.body.token");
    expect(expressionFor('$.items[0]["a b"]')).toBe('res.body.items[0]["a b"]');
  });

  it("suggests a variable name from the last key", () => {
    expect(variableNameFor("$.token")).toBe("token");
    expect(variableNameFor("$.user.id")).toBe("id");
    expect(variableNameFor('$.user["first name"]')).toBe("firstName");
    expect(variableNameFor("$.items[0]")).toBe("items0");
    expect(variableNameFor("$.items[0].id")).toBe("id");
    expect(variableNameFor('$["content-type"]')).toBe("contentType");
    expect(variableNameFor("$")).toBe("body");
    expect(variableNameFor("$[3]")).toBe("body3");
  });

  it("keeps text as it is and writes the rest as JSON", () => {
    expect(valueText("abc")).toBe("abc");
    expect(valueText(7)).toBe("7");
    expect(valueText(true)).toBe("true");
    expect(valueText(null)).toBe("null");
    expect(valueText({ a: [1] })).toBe('{"a":[1]}');
  });
});
