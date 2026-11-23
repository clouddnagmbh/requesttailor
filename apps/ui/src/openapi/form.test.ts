// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe as suite, expect, it } from "vitest";

import { bodyFromForm, describe, flatObject, formFromBody, generate, sample, validateJson, validateText } from "./form";

suite("form generation per schema type", () => {
  it("picks the field kind from type, enum and format", () => {
    expect(describe({ type: "string" }).kind).toBe("text");
    expect(describe({ type: "string", enum: ["a", "b"] })).toMatchObject({ kind: "enum", options: ["a", "b"] });
    expect(describe({ type: "boolean" })).toMatchObject({ kind: "boolean", options: ["true", "false"] });
    expect(describe({ type: "integer", minimum: 1, maximum: 100 }).kind).toBe("integer");
    expect(describe({ type: "number" }).kind).toBe("number");
    expect(describe({ type: "string", format: "date" })).toMatchObject({ kind: "date", generatable: true });
    expect(describe({ type: "string", format: "date-time" }).kind).toBe("date-time");
    expect(describe({ type: "string", format: "uuid" })).toMatchObject({ kind: "uuid", generatable: true });
    expect(describe({ type: "string", format: "email" }).kind).toBe("email");
    expect(describe({ type: "object", properties: {} }).kind).toBe("json");
    expect(describe({ type: "array", items: { type: "string" } }).kind).toBe("json");
  });

  it("merges allOf before choosing", () => {
    expect(describe({ allOf: [{ type: "string" }, { enum: ["x"] }] })).toMatchObject({ kind: "enum", options: ["x"] });
  });

  it("states constraints in plain words", () => {
    expect(describe({ type: "integer", minimum: 1, maximum: 100 }).constraints).toMatch(/1/);
    expect(describe({ type: "string", pattern: "^[a-z]+$" }).constraints).toMatch(/\^\[a-z\]\+\$/);
  });

  it("generates values for formats, enums and numbers", () => {
    const now = new Date(Date.UTC(2026, 10, 23, 7, 5, 9));
    expect(generate({ type: "string", format: "date" }, now)).toBe("2026-11-23");
    expect(generate({ type: "string", format: "date-time" }, now)).toBe("2026-11-23T07:05:09Z");
    expect(generate({ type: "string", format: "uuid" }, now, () => "u-1")).toBe("u-1");
    expect(generate({ type: "string", format: "email" })).toBe("user@example.com");
    expect(generate({ type: "string", enum: ["placed", "approved"] })).toBe("placed");
    expect(generate({ type: "integer", minimum: 5 })).toBe("5");
    expect(generate({ type: "integer", exclusiveMinimum: 5 })).toBe("6");
  });

  it("samples an object with its properties", () => {
    const s = { type: "object", required: ["id"], properties: { id: { type: "integer" }, name: { type: "string" }, tags: { type: "array", items: { type: "string" } } } };
    expect(sample(s)).toMatchObject({ id: expect.any(Number), name: expect.any(String) });
  });
});

suite("validation messages", () => {
  it("required and empty", () => {
    expect(validateText({ type: "string" }, "", true)).toBe("required");
    expect(validateText({ type: "string" }, "", false)).toBeNull();
  });
  it("enum", () => {
    expect(validateText({ type: "string", enum: ["a", "b"] }, "c", true)).toBe("must be one of a, b");
  });
  it("numbers and bounds", () => {
    expect(validateText({ type: "integer" }, "1.5", true)).toBe("must be a whole number");
    expect(validateText({ type: "number" }, "x", true)).toBe("must be a number");
    expect(validateText({ type: "integer", minimum: 1 }, "0", true)).toBe("must be at least 1");
    expect(validateText({ type: "integer", maximum: 10 }, "11", true)).toBe("must be at most 10");
    expect(validateText({ type: "integer", exclusiveMinimum: 0 }, "0", true)).toBe("must be more than 0");
    expect(validateText({ type: "integer", multipleOf: 5 }, "7", true)).toBe("must be a multiple of 5");
  });
  it("strings, pattern and formats", () => {
    expect(validateText({ type: "string", minLength: 3 }, "ab", true)).toBe("must be at least 3 characters");
    expect(validateText({ type: "string", maxLength: 2 }, "abc", true)).toBe("must be at most 2 characters");
    expect(validateText({ type: "string", pattern: "^[a-z]+$" }, "A1", true)).toBe("must match ^[a-z]+$");
    expect(validateText({ type: "string", format: "uuid" }, "nope", true)).toBe("must be a UUID");
    expect(validateText({ type: "string", format: "email" }, "a@", true)).toBe("must be an email address");
    expect(validateText({ type: "string", format: "date" }, "2026-13-01", true)).toBe("must be a date (YYYY-MM-DD)");
    expect(validateText({ type: "string", format: "uri" }, "example.com", true)).toBe("must be a full URL");
    expect(validateText({ type: "string", format: "date" }, "2026-11-23", true)).toBeNull();
  });
  it("leaves templates alone", () => {
    expect(validateText({ type: "integer" }, "{{petId}}", true)).toBeNull();
  });
  it("checks JSON against the schema with paths", () => {
    const s = { type: "object", required: ["petId"], properties: { petId: { type: "integer" }, status: { type: "string", enum: ["placed"] } } };
    const errors = validateJson(s, { status: "lost" });
    expect(errors.map((e) => e.path)).toEqual(expect.arrayContaining(["$", "$.status"]));
    expect(validateJson(s, { petId: "x" })[0]).toMatchObject({ path: "$.petId", message: expect.stringMatching(/expected integer/) });
    expect(validateJson(s, { petId: 1 })).toEqual([]);
  });
});

suite("body as a form", () => {
  const schema = {
    type: "object",
    required: ["petId"],
    properties: { petId: { type: "integer" }, gift: { type: "boolean" }, note: { type: "string" }, id: { type: "integer", readOnly: true } },
  };
  it("flat objects become form fields, read-only ones left out", () => {
    expect(flatObject(schema)?.map((p) => p.name)).toEqual(["petId", "gift", "note"]);
    expect(flatObject({ type: "object", properties: { inner: { type: "object" } } })).toBeNull();
  });
  it("form values become typed JSON and back", () => {
    const props = flatObject(schema)!;
    const body = bodyFromForm(props, { petId: "7", gift: "true", note: "" });
    expect(JSON.parse(body)).toEqual({ petId: 7, gift: true });
    expect(formFromBody(body, props)).toEqual({ petId: "7", gift: "true", note: "" });
    expect(formFromBody("[1]", props)).toBeNull();
  });
});
