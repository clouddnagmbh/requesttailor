// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { emptyRequest } from "../commands/mock";
import { mockAi } from "../commands/mock-ai";
import type { StoredResponse, TreeNode } from "../commands/types";
import { failed, folderOf, folders, usable, withProposal, withSuggestedTests } from "./apply";
import { inline, parseMarkdown } from "./markdown";

function stored(status: number, extra: Partial<StoredResponse["report"]> = {}): StoredResponse {
  return {
    report: {
      version: 1,
      request_path: "a.yml",
      environment: null,
      request: null,
      response: { status } as never,
      variables: [],
      unresolved: [],
      error: null,
      ...extra,
    },
    response_id: 1,
    size: 0,
    line_count: 0,
    is_text: true,
  };
}

describe("apply", () => {
  it("counts error statuses and failed checks as failed", () => {
    expect(failed(stored(200))).toBe(false);
    expect(failed(stored(404))).toBe(true);
    expect(failed(stored(200, { tests: [{ name: "t", passed: false, origin: "request" }] }))).toBe(true);
    expect(
      failed(stored(200, { assertions: [{ expression: "res.status", operator: "eq", expected: "1", actual: "2", passed: false }] })),
    ).toBe(true);
    expect(failed({ ...stored(500), response_id: null })).toBe(false);
    expect(failed(undefined)).toBe(false);
  });

  it("lists folders and the folder of a request", () => {
    const tree: TreeNode = {
      kind: "folder",
      name: "c",
      path: "",
      children: [
        { kind: "folder", name: "Users", path: "users", children: [{ kind: "folder", name: "Admin", path: "users/admin", children: [] }] },
        { kind: "request", name: "r", path: "r.yml", method: "GET" },
      ],
    };
    expect(folders(tree).map((f) => [f.path, f.label])).toEqual([
      ["", "(collection root)"],
      ["users", "Users"],
      ["users/admin", "Users / Admin"],
    ]);
    expect(folderOf("users/admin/x.yml")).toBe("users/admin");
    expect(folderOf("x.yml")).toBe("");
    expect(folderOf(null)).toBe("");
  });

  it("keeps the created request's name, position and auth", () => {
    const base = { ...emptyRequest("Get order"), seq: 3 };
    const proposal = { ...emptyRequest("Other"), method: "POST", url: "{{baseUrl}}/orders", auth: { type: "none" as const } };
    const spec = withProposal(base, proposal);
    expect([spec.name, spec.seq, spec.method, spec.url, spec.auth.type]).toEqual(["Get order", 3, "POST", "{{baseUrl}}/orders", "inherit"]);
  });

  it("appends suggested tests and skips assertions already there", () => {
    const spec = {
      ...emptyRequest("r"),
      scripts: [{ phase: "tests" as const, code: "test('a', () => {});" }],
      assertions: [{ expression: "res.status", operator: "eq", value: "200", description: null, enabled: true }],
    };
    const next = withSuggestedTests(spec, {
      text: "",
      scripts: [{ phase: "tests", code: "test('b', () => {});" }],
      assertions: [
        { expression: "res.status", operator: "eq", value: "200", description: "dup", enabled: true },
        { expression: "res.body", operator: "isJson", value: "", description: null, enabled: true },
      ],
    });
    expect(next.scripts).toEqual([{ phase: "tests", code: "test('a', () => {});\n\ntest('b', () => {});" }]);
    expect(next.assertions.map((a) => a.operator)).toEqual(["eq", "isJson"]);
    expect(spec.assertions).toHaveLength(1);
  });

  it("is usable only when set up", () => {
    expect(usable(null, true)).toBe(false);
    expect(usable({ provider: "anthropic", model: "claude-opus-5-5", base_url: null }, false)).toBe(false);
    expect(usable({ provider: "anthropic", model: "claude-opus-5-5", base_url: null }, true)).toBe(true);
    expect(usable({ provider: "openai-compatible", model: "llama3.1", base_url: "http://localhost:11434/v1" }, false)).toBe(true);
    expect(usable({ provider: "openai-compatible", model: "llama3.1", base_url: null }, false)).toBe(false);
  });
});

describe("markdown", () => {
  it("parses the subset into text, never HTML", () => {
    expect(inline("a **b** `c` *d* <i>")).toEqual([
      { text: "a " },
      { text: "b", bold: true },
      { text: " " },
      { text: "c", code: true },
      { text: " " },
      { text: "d", italic: true },
      { text: " <i>" },
    ]);
    const blocks = parseMarkdown("# Cause\nThe token\nexpired.\n\n- one\n- two\n\n1. fix\n\n```js\nx()\n```");
    expect(blocks.map((b) => b.kind)).toEqual(["h", "p", "ul", "ol", "code"]);
    expect(blocks[1]).toEqual({ kind: "p", spans: [{ text: "The token expired." }] });
    expect(blocks[4]).toEqual({ kind: "code", text: "x()" });
  });
});

describe("mockAi", () => {
  it("is off until configured, needs a key for Anthropic and answers per task", async () => {
    const ai = mockAi();
    const explain = { task: "explain-error" as const, path: "a.yml", response: 1 };
    const input = { environment: null, vars: [] };
    await expect(ai.ask(explain, input)).rejects.toMatchObject({ kind: "not-configured" });
    await ai.setConfig({ provider: "anthropic", model: "claude-opus-5-5", base_url: null });
    await expect(ai.ask(explain, input)).rejects.toMatchObject({ kind: "no-key" });
    await ai.setKey("anthropic", "sk");
    expect(await ai.hasKey("anthropic")).toBe(true);
    expect((await ai.ask(explain, input)).text).toContain("refused");
    const created = await ai.ask({ task: "create-request", description: "create an order", folder: "" }, input);
    expect([created.request?.method, created.request?.url, created.request?.auth.type]).toEqual(["POST", "{{baseUrl}}/order", "inherit"]);
    const tests = await ai.ask({ task: "suggest-tests", path: "a.yml", response: 1 }, input);
    expect(tests.scripts?.[0]?.phase).toBe("tests");
    expect(ai.asked.map((t) => t.task)).toEqual(["explain-error", "create-request", "suggest-tests"]);
  });
});
