// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";

describe("MockBackend", () => {
  it("opens, edits, saves and never writes secret values", async () => {
    const b = new MockBackend();
    await b.openCollection();
    await b.secrets.set("Local", "token", "s3cr3t-value");
    const spec = await b.getRequest("users/create-user.yml");
    await b.updateRequest("users/create-user.yml", { ...spec, url: "{{baseUrl}}/users/v2" });
    expect(await b.dirty()).toEqual(["users/create-user.yml"]);
    const sent = await b.send("users/create-user.yml", { environment: "Local", vars: [] });
    expect(JSON.stringify(sent.report.request)).not.toContain("s3cr3t-value");
    const written = await b.save();
    expect(written.map((f) => f.path)).toEqual(["users/create-user.yml"]);
    expect([...b.files.values()].join()).not.toContain("s3cr3t-value");
    expect(await b.dirty()).toEqual([]);
  });

  it("serves a virtual 100 MB body by lines", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const r = await b.send("demo/large.yml", { environment: "Local", vars: [] });
    expect(r.size).toBe(100 * 1024 * 1024);
    const last = await b.responseLines(r.response_id!, r.line_count - 1, 10);
    expect(last.lines).toHaveLength(1);
  });

  it("reports a blocked request until the companion is used", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const blocked = await b.send("demo/blocked.yml", { environment: null, vars: [] });
    expect(blocked.report.error).toEqual({ kind: "transport", message: { kind: "blocked", message: "Failed to fetch" } });
    await b.transport.useCompanion(true);
    const ok = await b.send("demo/blocked.yml", { environment: null, vars: [] });
    expect(ok.report.error).toBeNull();
  });
});
