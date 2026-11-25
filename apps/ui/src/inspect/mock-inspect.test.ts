// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { mockInspect, mockParseJwt } from "../commands/mock-inspect";

const part = (v: unknown) => btoa(JSON.stringify(v)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const jwt = (claims: Record<string, unknown>, alg = "RS256") => `${part({ alg })}.${part(claims)}.c2ln`;

describe("the mock mirror of the inspector", () => {
  it("reads claims, orders them like the core and warns", () => {
    const t = mockParseJwt(jwt({ zzz: 1, exp: 100, iss: "https://idp.example", sub: "a" }))!;
    expect(t.claims.map((c) => c.name)).toEqual(["iss", "sub", "exp", "zzz"]);
    expect(t.warnings.map((w) => w.kind)).toEqual(["expired"]);
    expect(t.signature_verified).toBe(false);
    expect(mockParseJwt(jwt({}, "none"))!.warnings.map((w) => w.kind)).toEqual(["algorithm-none"]);
    expect(mockParseJwt("nope")).toBeNull();
    expect(mockParseJwt("a.b.c")).toBeNull();
  });

  it("checks a stored mock response and honours the switches", async () => {
    const backend = new MockBackend();
    await backend.openCollection();
    const stored = await backend.send("users/list-users.yml", { environment: null, vars: [] });
    const target = { path: "users/list-users.yml", environment: null, response_id: stored.response_id };
    const first = await backend.inspect.checks(target);
    expect(first.map((o) => o.id)).toContain("chk/nosniff-missing");
    await backend.inspect.ignoreCheck("chk/nosniff-missing", "device");
    expect((await backend.inspect.checks(target)).map((o) => o.id)).not.toContain("chk/nosniff-missing");
    await expect(backend.inspect.ignoreCheck("chk/nope", "device")).rejects.toThrow();
  });

  it("without a host it answers the token reader and nothing else", async () => {
    const api = mockInspect();
    expect((await api.jwt(jwt({ exp: 1 }))).expires_at).toBe(1);
    expect(await api.checks({ path: "a.yml", response_id: 1 })).toEqual([]);
  });
});
