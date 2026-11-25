// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock mirror of the access check (`?backend=mock`): the same rules as the core around a
// server that leaks when the URL says `leak`.

import { describe, expect, it } from "vitest";

import { emptyRequest } from "../commands/mock";
import { mockAccess } from "../commands/mock-access";
import type { AgentPolicy, RequestSpec } from "../commands/types";

const policy: AgentPolicy = { access: "read", environments: [] };

function api(spec: RequestSpec) {
  return mockAccess({
    getRequest: async () => spec,
    policy: async () => policy,
    environments: async () => ["alice", "bob", "PRD"].map((name) => ({ name })),
  });
}

function request(method: string, url: string): RequestSpec {
  const s = emptyRequest("r");
  s.method = method;
  s.url = url;
  return s;
}

const AB = { a: "alice", b: "bob" };

describe("mock access check", () => {
  it("finds broken object access, ok and inconclusive", async () => {
    const leak = await api(request("GET", "{{base}}/leak/{{orderId}}")).check("r.yml", AB, {});
    expect(leak.verdict.kind).toBe("broken-object-access");
    expect(leak.findings[0]!.evidence.map((e) => e.profile)).toEqual(["A", "B"]);
    const ok = await api(request("GET", "{{base}}/orders/{{orderId}}")).check("r.yml", AB, {});
    expect(ok.verdict.kind).toBe("ok");
    expect(ok.status_b).toBe(403);
    const crash = await api(request("GET", "{{base}}/crash/{{orderId}}")).check("r.yml", AB, {});
    expect(crash.verdict.kind).toBe("inconclusive");
  });

  it("does not guess an object", async () => {
    const r = await api(request("GET", "{{base}}/health")).check("r.yml", AB, {});
    expect(r.verdict).toEqual({ kind: "skipped", reason: "no object reference found" });
    const f = await api(request("GET", "{{base}}/admin")).check("r.yml", AB, { b_must_be_denied: true });
    expect(f.verdict.kind).toBe("ok");
    const g = await api(request("GET", "{{base}}/leak/admin")).check("r.yml", AB, { b_must_be_denied: true });
    expect(g.verdict.kind).toBe("broken-function-access");
  });

  it("writes need permission and never go to production", async () => {
    const put = request("PUT", "{{base}}/leak/{{orderId}}");
    expect((await api(put).check("r.yml", AB, {})).verdict).toEqual({ kind: "skipped", reason: "skipped (writes)" });
    expect((await api(put).check("r.yml", AB, { allow_writes: true })).verdict.kind).toBe("broken-object-access");
    expect((await api(put).check("r.yml", { a: "alice", b: "PRD" }, { allow_writes: true })).verdict).toEqual({
      kind: "skipped",
      reason: "skipped (production)",
    });
    // A read may.
    expect((await api(request("GET", "{{base}}/leak/{{orderId}}")).check("r.yml", { a: "alice", b: "PRD" }, {})).verdict.kind).toBe("broken-object-access");
  });

  it("refuses the same or an unknown profile", async () => {
    const a = api(request("GET", "{{base}}/x/{{id}}"));
    await expect(a.check("r.yml", { a: "alice", b: "alice" }, {})).rejects.toThrow();
    await expect(a.check("r.yml", { a: "alice", b: "nobody" }, {})).rejects.toThrow();
  });

  it("shows no value but ids", async () => {
    const r = await api(request("GET", "{{base}}/leak/{{orderId}}")).check("r.yml", AB, {});
    const all = JSON.stringify(r);
    expect(all).not.toContain("@");
  });
});
