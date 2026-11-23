// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand A: `AppState` over several collections (mock backend: "Demo API" and
// "Billing API" in one root, "Add collection…" adds "Payments API").

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { BILLING_ID, DEMO_ID, PAYMENTS_ID } from "../commands/mock-collections";
import { AppState } from "./app.svelte";

async function opened() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  return { app, backend };
}

describe("AppState over several collections", () => {
  it("shows every collection of the folder, the first active, other trees read on demand", async () => {
    const { app } = await opened();
    expect(app.collections.map((c) => c.name)).toEqual(["Demo API", "Billing API"]);
    expect(app.roots.map((r) => r.collections)).toEqual([[DEMO_ID, BILLING_ID]]);
    expect(app.activeCollection).toBe(DEMO_ID);
    expect(Object.keys(app.trees)).toEqual([DEMO_ID]);
    await app.ensureTree(BILLING_ID);
    expect(Object.keys(app.trees).sort()).toEqual([BILLING_ID, DEMO_ID].sort());
    expect(app.activeCollection).toBe(DEMO_ID);
  });

  it("keeps tabs of two collections with the same path open and switches with the tab", async () => {
    const { app } = await opened();
    await app.selectRef({ collection: DEMO_ID, path: "health.yml" });
    await app.selectRef({ collection: BILLING_ID, path: "health.yml" });
    expect(app.openTabs).toEqual([
      { collection: DEMO_ID, path: "health.yml" },
      { collection: BILLING_ID, path: "health.yml" },
    ]);
    expect(app.activeCollection).toBe(BILLING_ID);
    expect(app.tabs).toEqual(["health.yml"]);
    expect(app.drafts["health.yml"].name).toBe("Billing health");
    await app.selectRef({ collection: DEMO_ID, path: "health.yml" });
    expect(app.collection?.name).toBe("Demo API");
    expect(app.drafts["health.yml"].name).toBe("Health");
    expect(app.active).toBe("health.yml");
  });

  it("follows the tab's collection with the environment, remembered per collection", async () => {
    const { app } = await opened();
    expect(app.environment).toBe("Local");
    app.setEnvironment("PRD");
    await app.activate(BILLING_ID);
    expect(app.environments.map((e) => e.name)).toEqual(["Staging", "Local"]);
    expect(app.environment).toBe("Staging");
    await app.activate(DEMO_ID);
    expect(app.environment).toBe("PRD");
  });

  it("saves the unsaved changes of every collection at once", async () => {
    const { app, backend } = await opened();
    await app.selectRef({ collection: DEMO_ID, path: "health.yml" });
    await app.edit("health.yml", { ...app.drafts["health.yml"], url: "http://demo.example/health" });
    await app.selectRef({ collection: BILLING_ID, path: "health.yml" });
    await app.edit("health.yml", { ...app.drafts["health.yml"], url: "http://billing.example/health" });
    expect(app.collections.map((c) => c.dirty)).toEqual([1, 1]);
    expect(app.dirty).toEqual(["health.yml"]);
    await app.save();
    expect(backend.files.get("health.yml")).toContain("demo.example");
    expect(backend.files.get("billing/health.yml")).toContain("billing.example");
    expect(app.collections.map((c) => c.dirty)).toEqual([0, 0]);
    expect(app.dirty).toEqual([]);
  });

  it("an edit in progress lands in its own collection when another becomes active", async () => {
    const { app, backend } = await opened();
    await app.selectRef({ collection: DEMO_ID, path: "health.yml" });
    const edit = app.edit("health.yml", { ...app.drafts["health.yml"], url: "http://only-demo" });
    const switching = app.activate(BILLING_ID);
    await Promise.all([edit, switching]);
    expect(app.activeCollection).toBe(BILLING_ID);
    expect(await backend.collections.dirty(DEMO_ID)).toEqual(["health.yml"]);
    expect(await backend.collections.dirty(BILLING_ID)).toEqual([]);
  });

  it("closing the last tab of a collection moves to the neighbouring tab of another", async () => {
    const { app } = await opened();
    await app.selectRef({ collection: DEMO_ID, path: "health.yml" });
    await app.selectRef({ collection: BILLING_ID, path: "invoices/list-invoices.yml" });
    await app.closeRef({ collection: BILLING_ID, path: "invoices/list-invoices.yml" });
    expect(app.openTabs).toEqual([{ collection: DEMO_ID, path: "health.yml" }]);
    expect(app.activeCollection).toBe(DEMO_ID);
    expect(app.active).toBe("health.yml");
    // Closing a tab of a collection that is not active leaves the active one alone.
    await app.selectRef({ collection: BILLING_ID, path: "health.yml" });
    await app.closeRef({ collection: DEMO_ID, path: "health.yml" });
    expect(app.activeCollection).toBe(BILLING_ID);
    expect(app.openTabs).toEqual([{ collection: BILLING_ID, path: "health.yml" }]);
  });

  it("adds a folder, removes it again, refuses while it has unsaved changes", async () => {
    const { app } = await opened();
    await app.addCollection();
    expect(app.collections.map((c) => c.name)).toEqual(["Demo API", "Billing API", "Payments API"]);
    expect(app.activeCollection).toBe(PAYMENTS_ID);
    await app.select("health.yml");
    await app.edit("health.yml", { ...app.drafts["health.yml"], url: "http://changed" });
    await app.removeRoot("mock://payments");
    expect(app.error).toMatch(/unsaved changes/);
    expect(app.collections).toHaveLength(3);
    await app.save();
    await app.removeRoot("mock://payments");
    expect(app.collections.map((c) => c.name)).toEqual(["Demo API", "Billing API"]);
    expect(app.activeCollection).toBe(DEMO_ID);
    expect(app.openTabs.every((t) => t.collection !== PAYMENTS_ID)).toBe(true);
  });

  it("creates a collection unsaved and makes it active", async () => {
    const { app } = await opened();
    const info = await app.createCollection({
      name: "Inventory",
      root: "mock://demo",
      dir: "inventory",
      base_url: "http://localhost:8080",
      environment: "Local",
    });
    expect(info?.name).toBe("Inventory");
    expect(app.collection?.name).toBe("Inventory");
    expect(app.collections.map((c) => c.name)).toEqual(["Demo API", "Billing API", "Inventory"]);
    expect(app.dirty.length).toBeGreaterThan(0);
    expect(app.environments.map((e) => e.name)).toEqual(["Local"]);
  });

  it("re-reads every collection of the repository on reload", async () => {
    const { app, backend } = await opened();
    await app.selectRef({ collection: BILLING_ID, path: "health.yml" });
    await app.activate(DEMO_ID);
    expect(app.collections.every((c) => c.repo === "mock://demo")).toBe(true);
    // Files change underneath in Billing (a checkout, as the Git panel sees it).
    await backend.collections.activate(BILLING_ID);
    await backend.createRequest("", "After checkout");
    await backend.collections.activate(DEMO_ID);
    await app.reload();
    const billing = app.trees[BILLING_ID];
    expect(billing.kind === "folder" && billing.children.map((c) => c.name)).toContain("After checkout");
  });
});
