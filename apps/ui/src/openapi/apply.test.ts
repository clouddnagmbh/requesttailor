// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { emptyRequest } from "../commands/mock";
import type { Environment, InputField, SendRequirements } from "../commands/types";
import { defaultLayer, initialState, planApply } from "./apply";

const field = (f: Partial<InputField> & Pick<InputField, "id" | "place" | "name">): InputField => ({
  required: false,
  secret: false,
  auth: false,
  value: null,
  problem: null,
  schema: null,
  description: null,
  example: null,
  ...f,
});

const missing = { kind: "missing" as const, message: "required" };

function requirements(fields: InputField[]): SendRequirements {
  return {
    operation: "GET /pets/{petId}",
    summary: null,
    deprecated: false,
    spec: "openapi.yaml",
    ready: false,
    fields,
    servers: [{ url: "https://{{region}}.example.com", description: null }],
  };
}

describe("apply", () => {
  it("puts baseUrl, auth and server variables in the environment by default", () => {
    const servers = requirements([]).servers;
    expect(defaultLayer(field({ id: "variable:baseUrl", place: "variable", name: "baseUrl" }), servers)).toBe("environment");
    expect(defaultLayer(field({ id: "variable:token", place: "variable", name: "token", auth: true }), servers)).toBe("environment");
    expect(defaultLayer(field({ id: "variable:region", place: "variable", name: "region" }), servers)).toBe("environment");
    expect(defaultLayer(field({ id: "variable:other", place: "variable", name: "other" }), servers)).toBe("request");
  });

  it("fills path, query, header and body on the request, unsaved like any edit", () => {
    const spec = { ...emptyRequest("Pet"), url: "{{baseUrl}}/pets/:petId" };
    const req = requirements([
      field({ id: "path:petId", place: "path", name: "petId", required: true, value: "", problem: missing }),
      field({ id: "query:limit", place: "query", name: "limit", problem: missing }),
      field({ id: "header:X-Trace", place: "header", name: "X-Trace", problem: missing }),
      field({ id: "body", place: "body", name: "body", problem: missing }),
    ]);
    const state = initialState(req, null);
    state.values["path:petId"] = "42";
    state.values["query:limit"] = "10";
    state.values["header:X-Trace"] = "t-1";
    state.values.body = '{"a":1}';
    const plan = planApply(spec, req, state, []);
    expect(plan.changed).toBe(true);
    expect(plan.environment).toBeNull();
    expect(plan.spec.params).toEqual(expect.arrayContaining([expect.objectContaining({ kind: "path", name: "petId", value: "42" })]));
    expect(plan.spec.url).toContain("limit=10");
    expect(plan.spec.headers).toEqual(
      expect.arrayContaining([
        { name: "X-Trace", value: "t-1", enabled: true },
        { name: "Content-Type", value: "application/json", enabled: true },
      ]),
    );
    expect(plan.spec.body).toEqual({ type: "raw", kind: "json", data: '{"a":1}' });
    // The input spec is untouched.
    expect(spec.params).toEqual([]);
  });

  it("writes variables to the chosen environment and secrets only to the secret list", () => {
    const envs: Environment[] = [{ name: "Staging", variables: [{ name: "keep", value: "1", enabled: true, secret: false }], client_certificates: [], external_secrets: null }];
    const req = requirements([
      field({ id: "variable:baseUrl", place: "variable", name: "baseUrl", problem: missing }),
      field({ id: "variable:token", place: "variable", name: "token", secret: true, auth: true, problem: missing }),
      field({ id: "variable:note", place: "variable", name: "note", problem: missing }),
    ]);
    const state = initialState(req, "Staging");
    state.values["variable:baseUrl"] = "https://staging.example.com";
    state.values["variable:token"] = "s3cret";
    state.values["variable:note"] = "hi";
    const plan = planApply(emptyRequest("x"), req, state, envs);
    expect(plan.environment?.name).toBe("Staging");
    expect(plan.environment?.variables).toEqual([
      { name: "keep", value: "1", enabled: true, secret: false },
      { name: "baseUrl", value: "https://staging.example.com", enabled: true, secret: false },
      { name: "token", value: null, enabled: true, secret: true },
    ]);
    expect(JSON.stringify(plan.environment)).not.toContain("s3cret");
    expect(plan.secrets).toEqual([{ environment: "Staging", name: "token", value: "s3cret" }]);
    expect(plan.spec.vars).toEqual([{ name: "note", value: "hi", enabled: true, secret: false }]);
  });

  it("creates a new environment by name and blocks without one", () => {
    const req = requirements([field({ id: "variable:baseUrl", place: "variable", name: "baseUrl", problem: missing })]);
    const state = initialState(req, "Production");
    state.values["variable:baseUrl"] = "https://api.example.com";
    expect(planApply(emptyRequest("x"), req, state, []).environment?.name).toBe("Production");
    const none = { ...state, environment: null };
    expect(planApply(emptyRequest("x"), req, none, []).blocked).toMatch(/environment/);
  });

  it("changes nothing for fields left as they were", () => {
    const req = requirements([field({ id: "query:limit", place: "query", name: "limit" })]);
    const plan = planApply(emptyRequest("x"), req, initialState(req, null), []);
    expect(plan.changed).toBe(false);
  });
});
