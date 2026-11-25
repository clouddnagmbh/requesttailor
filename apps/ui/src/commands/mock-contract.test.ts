// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock's contract API (wave 12, strand F): no spec until the demo is switched on, then
// scripted answers that follow the shape of the real ones.

import { describe, expect, it } from "vitest";

import { BackendError } from "./backend";
import { MockBackend } from "./mock";
import type { MockContract } from "./mock-contract";

async function demo(on: boolean) {
  const b = new MockBackend();
  await b.openCollection();
  const c = b.contract as MockContract;
  c.enableDemo(on);
  return c;
}

describe("mock contract", () => {
  it("has no spec by default and refuses to check", async () => {
    const c = await demo(false);
    expect(await c.spec()).toBeNull();
    await expect(c.check(null)).rejects.toBeInstanceOf(BackendError);
    await expect(c.changes("HEAD", "HEAD")).rejects.toMatchObject({ kind: "format" });
  });

  it("checks, lists changes and writes a changelog with the demo spec", async () => {
    const c = await demo(true);
    expect(await c.spec()).toMatchObject({ title: "Orders API", version: "2.0.0", configured: false });
    const report = await c.check(null);
    expect(report.covered).toBeLessThan(report.operations);
    expect(report.findings.map((f) => f.kind)).toContain("no-operation");
    const changes = await c.changes("HEAD", "HEAD");
    expect(changes.changes.filter((x) => x.breaking)).toHaveLength(3);
    await expect(c.changes("nope", "HEAD")).rejects.toBeInstanceOf(BackendError);
    const text = await c.changelog("HEAD", "HEAD");
    expect(text).toContain("## Breaking changes");
    expect(text).toContain("`DELETE /orders/{id}`");
  });

  it("has GraphQL and Protobuf changes with the demo, none without (a hint is never an error)", async () => {
    const off = await demo(false);
    expect((await off.schemaChanges("graphql", "HEAD", "HEAD")).changes).toEqual([]);
    const c = await demo(true);
    const graphql = await c.schemaChanges("graphql", "HEAD", "HEAD");
    expect(graphql.changes.filter((x) => x.breaking).map((x) => x.operation)).toEqual(["Order.total", "Query.orders"]);
    const protobuf = await c.schemaChanges("protobuf", "HEAD", "HEAD");
    expect(protobuf.changes.filter((x) => x.breaking)).toHaveLength(1);
    await expect(c.schemaChanges("graphql", "nope", "HEAD")).rejects.toBeInstanceOf(BackendError);
    const text = await c.changelog("HEAD", "HEAD");
    expect(text.indexOf("## GraphQL")).toBeGreaterThan(text.indexOf("## Other changes"));
    expect(text.indexOf("## Protobuf")).toBeGreaterThan(text.indexOf("## GraphQL"));
  });
});
