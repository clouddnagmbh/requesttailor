// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { mockActions, mockEvaluate } from "./mock-assert";
import type { Assertion, VarAction } from "./types";

const a = (expression: string, operator: string, value: string, enabled = true): Assertion => ({
  expression,
  operator,
  value,
  description: null,
  enabled,
});

const action = (expression: string, variable: string, method = "jsonq"): VarAction => ({
  phase: "after-response",
  expression,
  method,
  variable,
  scope: "runtime",
  enabled: true,
});

const response = { status: 200, body: JSON.stringify({ token: "t-1", user: { id: 7 }, items: [{ id: 1 }] }) };

describe("mock assertions", () => {
  it("evaluates status and JSON paths with eq and neq", () => {
    const results = mockEvaluate(
      [
        a("res.status", "eq", "200"),
        a("res.body.user.id", "eq", "7"),
        a("res.body.items[0].id", "eq", "1"),
        a("res.body.token", "eq", "{{expected}}"),
        a("res.status", "neq", "200"),
        a("res.body.nope", "eq", "1"),
        a("res.status", "gt", "100"),
        a("res.status", "eq", "500", false),
      ],
      response,
      (name) => (name === "expected" ? "t-1" : undefined),
    );
    expect(results.map((r) => r.passed)).toEqual([true, true, true, true, false, false, false]);
    expect(results[3].expected).toBe("t-1");
    expect(results[5].error).toContain("did not resolve");
    expect(results[6].error).toContain("only eq and neq");
  });

  it("takes values into variables", () => {
    expect(
      mockActions([action("res.body.token", "token"), action("res.body.user", "user"), action("", "code", "status"), action("res.body.x", "x")], response),
    ).toEqual([
      { name: "token", value: "t-1", scope: "runtime" },
      { name: "user", value: '{"id":7}', scope: "runtime" },
      { name: "code", value: "200", scope: "runtime" },
    ]);
  });
});

describe("mock send with session variables", () => {
  it("chains an action's value into the next send and keeps it per environment", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const login = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", {
      ...login,
      url: "{{baseUrl}}/login",
      assertions: [a("res.status", "eq", "200"), a("res.body.token", "eq", "mock-token-123")],
      actions: [action("res.body.token", "sessionToken")],
    });
    const sent = await b.send("health.yml", { environment: "Local", vars: [] });
    expect(sent.report.assertions?.every((r) => r.passed)).toBe(true);
    expect(sent.report.var_changes).toEqual([{ name: "sessionToken", value: "mock-token-123", scope: "runtime" }]);
    expect(await b.scripts.sessionVars("Local")).toEqual(sent.report.var_changes);
    expect(await b.scripts.sessionVars("PRD")).toEqual([]);

    const users = await b.getRequest("users/list-users.yml");
    await b.updateRequest("users/list-users.yml", {
      ...users,
      headers: [{ name: "X-Token", value: "{{sessionToken}}", enabled: true }],
    });
    const next = await b.send("users/list-users.yml", { environment: "Local", vars: [] });
    expect(next.report.request?.headers).toContainEqual(["X-Token", "mock-token-123"]);
    expect(next.report.variables).toContainEqual({ name: "sessionToken", source: { layer: "runtime" }, secret: false });
    const preview = await b.preview("users/list-users.yml", { environment: "Local", vars: [] });
    expect(preview.variables.find((v) => v.name === "sessionToken")?.value).toBe("mock-token-123");
    // The caller's own vars win.
    const own = await b.send("users/list-users.yml", { environment: "Local", vars: [["sessionToken", "typed"]] });
    expect(own.report.request?.headers).toContainEqual(["X-Token", "typed"]);
    // Another environment has its own session.
    const prd = await b.send("users/list-users.yml", { environment: "PRD", vars: [] });
    expect(prd.report.unresolved).toContain("sessionToken");
  });

  it("adds script outcomes for UI tests, and a script error stops before the response", async () => {
    const b = new MockBackend();
    await b.openCollection();
    b.scriptOutcome = () => ({ tests: [{ name: "t", passed: true, origin: "request" }] });
    const ok = await b.send("health.yml", { environment: "Local", vars: [] });
    expect(ok.report.tests).toHaveLength(1);
    expect(ok.response_id).not.toBeNull();
    b.scriptOutcome = () => ({ error: { kind: "script", message: "boom" } });
    const failed = await b.send("health.yml", { environment: "Local", vars: [] });
    expect(failed.report.error).toEqual({ kind: "script", message: "boom" });
    expect(failed.response_id).toBeNull();
    expect(failed.report.request).toBeNull();
  });
});
