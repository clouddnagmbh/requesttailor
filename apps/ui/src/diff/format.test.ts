// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { fieldLabel, fieldValueText, secretNote } from "./format";

describe("fieldValueText", () => {
  it("reads an assertion as text", () => {
    const v = { kind: "json", value: { operator: "eq", value: "200", enabled: true } } as const;
    expect(fieldValueText(["assertions", "res.status"], v)).toBe("res.status eq 200");
    expect(fieldValueText(["assertions", "res.status#2"], { kind: "json", value: { ...v.value, enabled: false } })).toBe("res.status eq 200 (off)");
  });

  it("drops the empty value of a unary operator", () => {
    const v = { kind: "json", value: { operator: "isJson", value: "", enabled: true } } as const;
    expect(fieldValueText(["assertions", "res.body"], v)).toBe("res.body isJson");
  });

  it("reads an action as text", () => {
    const v = { kind: "json", value: { phase: "after-response", expression: "res.body.token", method: "jsonq", scope: "runtime", enabled: true } } as const;
    expect(fieldValueText(["actions", "token"], v)).toBe("set runtime variable token from jsonq res.body.token");
  });

  it("leaves text and other JSON as before", () => {
    expect(fieldValueText(["scripts", "tests"], { kind: "text", text: "test()" })).toBe("test()");
    expect(fieldValueText(["auth"], { kind: "json", value: { type: "basic" } })).toBe('{"type":"basic"}');
    expect(fieldValueText(["assertions", "x"], null)).toBeNull();
  });
});

describe("secretNote", () => {
  const path = ["variables", "token", "@file-value"];
  it("says a secret-looking value was written into the environment file, without a value", () => {
    const note = secretNote(path, null, { kind: "secret" });
    expect(note).toContain("looks like a secret");
    expect(note).toContain("added");
    expect(note).toContain("token");
    expect(fieldLabel(path)).toBe("variables › token › value in the file");
  });
  it("keeps the short word for other secrets", () => {
    expect(secretNote(["headers", "Authorization"], { kind: "secret" }, { kind: "secret" })).toBe("changed");
  });
});
