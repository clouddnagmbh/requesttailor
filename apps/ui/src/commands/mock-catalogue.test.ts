// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";

const SOURCE = "https://plugins.example.test/plugins.json";

describe("mock catalogue", () => {
  it("has no built-in index: nothing is listed until a source is added and refreshed", async () => {
    const { catalogue } = new MockBackend();
    expect(await catalogue.sources()).toEqual([]);
    expect(await catalogue.refresh()).toEqual([]);
    await catalogue.addSource(SOURCE);
    expect((await catalogue.refresh())[0]?.index.plugins.length).toBeGreaterThan(0);
  });

  it("refuses plain http and other schemes as a source", async () => {
    const { catalogue } = new MockBackend();
    await expect(catalogue.addSource("http://example.test/plugins.json")).rejects.toThrow(/https/);
    await expect(catalogue.addSource("ftp://example.test/plugins.json")).rejects.toThrow(/https/);
    await expect(catalogue.addSource("http://127.0.0.1:48629/plugins.json")).resolves.toBeTruthy();
  });

  it("takes the rights from the module and warns about a difference", async () => {
    const { catalogue } = new MockBackend();
    await catalogue.addSource(SOURCE);
    const plan = await catalogue.plan(SOURCE, "sneaky");
    expect(plan.permissions.network).toEqual(["*"]);
    expect(plan.permissions_differ).toBe(true);
    expect(plan.warnings.length).toBe(1);
  });

  it("refuses a checksum mismatch and installs nothing", async () => {
    const backend = new MockBackend();
    await backend.catalogue.addSource(SOURCE);
    await expect(backend.catalogue.plan(SOURCE, "flaky")).rejects.toThrow(/checksum does not match/);
    expect(await backend.catalogue.installed()).toEqual([]);
  });

  it("installs through the plugin host and remembers where the plugin came from", async () => {
    const backend = new MockBackend();
    await backend.catalogue.addSource(SOURCE);
    const plan = await backend.catalogue.plan(SOURCE, "json-tools");
    expect(plan.update).toBe(false);
    const got = await backend.catalogue.install(plan);
    expect(got).toMatchObject({ plugin: "json-tools", version: "1.2.0", source: SOURCE });
    expect((await backend.plugins.list()).map((p) => p.manifest.id)).toContain("json-tools");
    expect((await backend.catalogue.installed())[0]?.plugin).toBe("json-tools");
    expect((await backend.catalogue.plan(SOURCE, "json-tools")).update).toBe(true);
  });
});
