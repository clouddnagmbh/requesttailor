// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock mirror of the minimal-case search (`?backend=mock`): the same limits and refusals as
// the core, over a server that fails while the request holds `boom`.

import { describe, expect, it } from "vitest";

import { emptyRequest } from "../commands/mock";
import { mockShrink } from "../commands/mock-shrink";
import type { AgentPolicy, RequestSpec } from "../commands/types";

const policy: AgentPolicy = { access: "read", environments: [] };

function api(spec: RequestSpec) {
  return mockShrink({ getRequest: async () => spec, policy: async () => policy });
}

function wide(): RequestSpec {
  const spec = emptyRequest("wide");
  spec.url = "http://x.test/items?" + Array.from({ length: 30 }, (_, i) => (i === 17 ? "q17=boom" : `q${i}=v${i}`)).join("&");
  spec.headers = [
    { name: "Authorization", value: "Bearer {{token}}", enabled: true },
    ...Array.from({ length: 30 }, (_, i) => ({ name: `X-H${i}`, value: `v${i}`, enabled: true })),
  ];
  return spec;
}

describe("mock shrink", () => {
  it("keeps the cause and the authorization, takes the rest away", async () => {
    const r = await api(wide()).run({ path: "a.yml", predicate: { kind: "server-error" } });
    expect(r.request.headers.map((h) => h.name)).toEqual(["Authorization"]);
    expect(r.request.url).toBe("http://x.test/items?q17=boom");
    expect(r.removed).toHaveLength(30 + 29);
    expect(r.one_minimal).toBe(true);
    expect(r.probes).toBeLessThan(40);
  });

  it("cuts a JSON body as a tree", async () => {
    const spec = emptyRequest("b");
    spec.url = "http://x.test/o";
    spec.body = { type: "raw", kind: "json", data: JSON.stringify({ a: 1, items: [{ sku: "x" }, { sku: "boom", qty: 2 }], z: { k: 1 } }) };
    const r = await api(spec).run({ path: "b.yml", predicate: { kind: "status", status: 500 } });
    const body = r.request.body;
    expect(body.type === "raw" && JSON.parse(body.data)).toEqual({ items: [{ sku: "boom" }] });
  });

  it("honours keep, the probe limit and a failure that does not show", async () => {
    const kept = await api(wide()).run({ path: "a.yml", predicate: { kind: "server-error" }, keep: ["header:X-H3"] });
    expect(kept.request.headers.map((h) => h.name)).toEqual(["Authorization", "X-H3"]);
    const limited = await api(wide()).run({ path: "a.yml", predicate: { kind: "server-error" }, max_probes: 4 });
    expect(limited.stopped_by).toBe("probes");
    expect(limited.probes).toBe(4);
    const plain = emptyRequest("ok");
    plain.url = "http://x.test/fine";
    expect((await api(plain).run({ path: "p.yml", predicate: { kind: "server-error" } })).not_reproduced).toBe(true);
  });

  it("asks for writes and refuses production", async () => {
    const post = wide();
    post.method = "POST";
    await expect(api(post).run({ path: "a.yml", predicate: { kind: "server-error" } })).rejects.toThrow(/requests that write/);
    await expect(api(post).run({ path: "a.yml", predicate: { kind: "server-error" }, confirmed_writes: true, environment: "PRD" })).rejects.toThrow(/production/);
    const ok = await api(post).run({ path: "a.yml", predicate: { kind: "server-error" }, confirmed_writes: true, environment: "Local" });
    expect(ok.one_minimal).toBe(true);
  });
});
