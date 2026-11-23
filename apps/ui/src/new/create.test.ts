// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand B: "Create" in the "New…" dialog against the mock backend: everything lands
// unsaved in the chosen target.

import { describe, expect, it, vi } from "vitest";

import type { CollectionInfo } from "../commands/backend";
import { MockBackend } from "../commands/mock";
import type { NewCollection } from "../commands/types";
import { AppState } from "../state/app.svelte";
import { createNew } from "./create";
import { initialForm, type CollectionForm, type FormContext, type McpTestForm, type RequestForm } from "./forms";

async function setup() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  const ctx: FormContext = { collection: app.activeCollection, folder: "", collections: app.collections, desktop: false };
  return { backend, app, ctx };
}

describe("createNew", () => {
  it("creates a request of every kind in the chosen folder, unsaved, and opens it", async () => {
    const { app, ctx } = await setup();
    const cases: [RequestForm["kind"], string, string][] = [
      ["http", "PUT", "users/rename-user.yml"],
      ["graphql", "POST", "users/rename-user-2.yml"],
      ["grpc", "GRPC", "users/rename-user-3.yml"],
      ["websocket", "WS", "users/rename-user-4.yml"],
      ["sse", "GET", "users/rename-user-5.yml"],
    ];
    for (const [kind, method, path] of cases) {
      const form = { ...(initialForm(kind, ctx) as RequestForm), name: "Rename user", folder: "users", method: "PUT", url: "{{baseUrl}}/x" };
      expect(await createNew(app, form), kind).toEqual({ ok: true });
      expect(app.active).toBe(path);
      expect(app.drafts[path]).toMatchObject({ name: "Rename user", method, url: "{{baseUrl}}/x" });
      expect(app.dirty).toContain(path);
    }
    expect(app.drafts["users/rename-user-5.yml"].headers).toContainEqual({ name: "Accept", value: "text/event-stream", enabled: true });
    expect(await app.backend.getRequest("users/rename-user-4.yml")).toMatchObject({ protocol: { type: "websocket" } });
  });

  it("creates a folder inside the target folder, never one that exists", async () => {
    const { app, backend, ctx } = await setup();
    const createFolder = vi.spyOn(backend, "createFolder");
    const form = { ...initialForm("folder", ctx), folder: "users", name: "Admin" };
    expect(await createNew(app, form)).toEqual({ ok: true });
    expect(createFolder).toHaveBeenCalledWith("users/Admin");
    expect(await createNew(app, { ...form, folder: "", name: "Users" })).toEqual({ ok: false, error: "There is already a folder “Users” here." });
    expect(createFolder).toHaveBeenCalledTimes(1);
  });

  it("creates an environment as a copy without secret values and switches to it", async () => {
    const { app, ctx } = await setup();
    const form = { ...initialForm("environment", ctx), name: "Staging", copyFrom: "Local" };
    expect(await createNew(app, form)).toEqual({ ok: true });
    const staging = app.environments.find((e) => e.name === "Staging")!;
    const local = app.environments.find((e) => e.name === "Local")!;
    expect(staging.variables.map((v) => v.name)).toEqual(local.variables.map((v) => v.name));
    expect(staging.variables.filter((v) => v.secret).every((v) => v.value === null)).toBe(true);
    expect(app.environment).toBe("Staging");
    expect(app.dirty).toContain("environments/Staging.yml");
    expect(app.view?.view).toEqual({ kind: "environments" });
    expect(await createNew(app, { ...form, name: "staging" })).toMatchObject({ ok: false });
  });

  it("stages an MCP test and opens the MCP client", async () => {
    const { app, ctx } = await setup();
    const form: McpTestForm = { ...(initialForm("mcp-test", ctx) as McpTestForm), name: "Echo", url: "http://localhost:3001/mcp" };
    expect(await createNew(app, form)).toEqual({ ok: true });
    expect(await app.backend.mcp.tests()).toContain("mcp/echo.mcp.yaml");
    expect(await app.backend.mcp.getTest("mcp/echo.mcp.yaml")).toMatchObject({ server: { transport: "http", url: "http://localhost:3001/mcp" }, tool: "" });
    expect(app.view?.view).toEqual({ kind: "mcp", test: "mcp/echo.mcp.yaml" });
  });

  it("creates a collection through the collection set; a cancelled picker keeps the dialog", async () => {
    const { app, backend, ctx } = await setup();
    const info: CollectionInfo = { id: "shop", name: "Shop", location: "shop" };
    const seen: NewCollection[] = [];
    let answer: CollectionInfo | null = info;
    vi.spyOn(backend.collections, "create").mockImplementation(async (spec) => {
      seen.push(spec);
      return answer;
    });
    const form: CollectionForm = { ...(initialForm("collection", ctx) as CollectionForm), name: "Shop", dir: "shop", baseUrl: "http://localhost:3000" };
    expect(await createNew(app, form)).toEqual({ ok: true });
    expect(seen[0]).toEqual({ name: "Shop", root: ctx.collections[0].root, dir: "shop", base_url: "http://localhost:3000", environment: "Local" });
    expect(app.collection?.name).toBe("Shop");

    answer = null;
    expect(await createNew(app, { ...form, place: "elsewhere" })).toEqual({ ok: false, cancelled: true });
    expect(seen[1]).toMatchObject({ root: null, dir: "" });
  });

  it("reports what the backend refuses", async () => {
    const { app, ctx } = await setup();
    // A collection that is not in the set cannot be activated.
    const form = { ...(initialForm("http", ctx) as RequestForm), collection: "elsewhere" };
    const out = await createNew(app, form);
    expect(out.ok).toBe(false);
    // A collection without a name is refused by the collection set.
    const collection = await createNew(app, { ...(initialForm("collection", ctx) as CollectionForm), name: "", dir: "x" });
    expect(collection).toMatchObject({ ok: false, error: expect.stringMatching(/name/i) });
  });
});
