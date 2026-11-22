// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { mockMcp } from "../commands/mock-mcp-client";
import { configOf, draftOf, draftProblem, emptyDraft, suggestedAssertions } from "./model";
import { fieldsOf, initialArgs, missing, parseField, parseRaw, withArg } from "./schema";

describe("argument form from a JSON Schema", () => {
  const schema = {
    type: "object",
    properties: {
      text: { type: "string", description: "What to echo" },
      n: { type: "integer", default: 3 },
      ratio: { type: ["number", "null"] },
      loud: { type: "boolean" },
      mode: { enum: ["a", "b"] },
      tags: { type: "array", items: { type: "string" } },
    },
    required: ["text", "loud"],
  };

  it("has a field per top-level property", () => {
    const fields = fieldsOf(schema)!;
    expect(fields.map((f) => [f.name, f.kind, f.required])).toEqual([
      ["text", "string", true],
      ["n", "integer", false],
      ["ratio", "number", false],
      ["loud", "boolean", true],
      ["mode", "enum", false],
      ["tags", "json", false],
    ]);
    expect(fields[0]!.description).toBe("What to echo");
    expect(fields[4]!.options).toEqual(["a", "b"]);
    expect(initialArgs(fields)).toEqual({ n: 3, loud: false });
    expect(missing(fields, { loud: false })).toEqual(["text"]);
  });

  it("has no form for schemas that are not objects", () => {
    expect(fieldsOf({ type: "string" })).toBeNull();
    expect(fieldsOf(null)).toBeNull();
    expect(fieldsOf({ type: "object" })).toEqual([]);
  });

  it("parses typed values and keeps variables", () => {
    const [, n, ratio, , , tags] = fieldsOf(schema)!;
    expect(parseField(n!, "4")).toEqual({ value: 4, error: null });
    expect(parseField(n!, "4.5").error).toBe("not an integer");
    expect(parseField(n!, "{{count}}")).toEqual({ value: "{{count}}", error: null });
    expect(parseField(ratio!, "0.5").value).toBe(0.5);
    expect(parseField(tags!, '["x"]').value).toEqual(["x"]);
    expect(parseField(tags!, "[").error).toBe("not valid JSON");
    expect(parseField(n!, "").value).toBeUndefined();
    expect(withArg({ a: 1, b: 2 }, "a", undefined)).toEqual({ b: 2 });
  });

  it("checks raw JSON", () => {
    expect(parseRaw('{"a":1}')).toEqual({ args: { a: 1 }, error: null });
    expect(parseRaw("").args).toEqual({});
    expect(parseRaw("[1]").error).toBe("the arguments must be a JSON object");
    expect(parseRaw("{").args).toBeNull();
  });
});

describe("server drafts and saved tests", () => {
  it("round-trips both transports", () => {
    const http = { transport: "http" as const, url: "{{mcpUrl}}", headers: [["Authorization", "Bearer {{token}}"]] as [string, string][] };
    expect(configOf(draftOf(http))).toEqual(http);
    const stdio = { transport: "stdio" as const, command: "npx", args: ["-y", "server"], env: [["TOKEN", "{{token}}"]] as [string, string][], cwd: null };
    expect(configOf(draftOf(stdio))).toEqual(stdio);
    expect(draftProblem(emptyDraft())).toBe("Enter the server URL.");
  });

  it("suggests assertions from the result", () => {
    expect(suggestedAssertions({ content: [], structured: { sum: 5, "a-b": 1, label: "x" }, is_error: false, time_ms: 1 }).map((a) => [a.expression, a.value])).toEqual([
      ["res.status", "200"],
      ["res.body.sum", "5"],
      ["res.body.label", '"x"'],
    ]);
    expect(suggestedAssertions({ content: [], structured: null, is_error: true, time_ms: 1 })[0]!.value).toBe("500");
  });

  it("the mock connects, calls, stages and runs tests", async () => {
    const mcp = mockMcp({ lookup: (_input, name) => (name === "word" ? "hi" : undefined) });
    const input = { environment: null, vars: [] };
    await expect(mcp.connect({ transport: "http", url: "http://unreachable/mcp", headers: [] }, input)).rejects.toThrow(/refused/);
    await expect(mcp.connect({ transport: "stdio", command: "nope", args: [], env: [], cwd: null }, input)).rejects.toThrow(/not found/);
    const { id } = await mcp.connect({ transport: "http", url: "http://localhost/mcp", headers: [] }, input);
    expect((await mcp.tools(id)).map((t) => t.name)).toEqual(["echo", "add", "fail", "image"]);
    expect((await mcp.call(id, "echo", { text: "{{word}}" })).content).toEqual([{ type: "text", text: "hi" }]);
    expect((await mcp.call(id, "add", { a: 2, b: 3 })).structured).toEqual({ sum: 5 });

    const test = {
      name: "Add",
      server: { transport: "http" as const, url: "http://localhost/mcp", headers: [] },
      tool: "add",
      arguments: { a: 2, b: 3 },
      assertions: [{ expression: "res.body.sum", operator: "eq", value: "5", description: null, enabled: true }],
    };
    const path = await mcp.saveTest(null, test);
    expect(path).toBe("mcp/add.mcp.yaml");
    expect(await mcp.saveTest(null, test)).toBe("mcp/add-2.mcp.yaml");
    expect(mcp.dirty()).toEqual(["mcp/add-2.mcp.yaml", "mcp/add.mcp.yaml"]);
    expect((await mcp.runTest(path, input)).passed).toBe(true);
    await mcp.saveTest(path, { ...test, assertions: [{ ...test.assertions[0]!, value: "6" }] });
    expect((await mcp.runTest(path, input)).passed).toBe(false);
    expect(mcp.save().map((c) => c.path)).toEqual(["mcp/add-2.mcp.yaml", "mcp/add.mcp.yaml"]);
    expect(mcp.dirty()).toEqual([]);
    expect(await mcp.tests()).toEqual(["mcp/add-2.mcp.yaml", "mcp/add.mcp.yaml"]);
  });
});
