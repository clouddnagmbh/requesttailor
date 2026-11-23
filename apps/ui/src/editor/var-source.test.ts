// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { createVarSource, folderOf, withEnvironmentVariable, type VarSourceApp } from "./var-source";

async function app(environment: string | null = "Local") {
  const backend = new MockBackend();
  const collection = await backend.openCollection();
  const state: VarSourceApp & { reloads: number } = {
    backend,
    collection,
    environment,
    dirty: [],
    reloads: 0,
    async reloadEnvironments() {
      state.reloads++;
      state.dirty = await backend.dirty();
    },
  };
  return state;
}

describe("createVarSource", () => {
  it("caches the scope per path and environment", async () => {
    const a = await app();
    let path = "users/list-users.yml";
    const source = createVarSource(a, { path: () => path, folder: () => folderOf(path) });
    expect(source.cached()).toBeNull();
    const loaded = await source.load();
    expect(source.cached()).toBe(loaded);
    expect(loaded.variables.some((v) => v.name === "userId")).toBe(true);
    path = "health.yml";
    expect(source.cached()).toBeNull();
    expect((await source.load()).variables.some((v) => v.name === "userId")).toBe(false);
  });

  it("offers environment, collection and the request's folder", async () => {
    const a = await app();
    const source = createVarSource(a, { path: () => "users/list-users.yml", folder: () => "users" });
    expect(source.targets().map((t) => [t.kind, t.detail, t.secretAllowed])).toEqual([
      ["environment", "Local", true],
      ["collection", "Demo API", false],
      ["folder", "users", false],
    ]);
    const none = createVarSource(await app(null), { path: () => "health.yml", folder: () => folderOf("health.yml") });
    expect(none.targets().map((t) => t.kind)).toEqual(["collection"]);
  });

  it("creates a secret in the environment through the secret store, never in the file", async () => {
    const a = await app();
    let changed = 0;
    const source = createVarSource(a, { path: () => "health.yml", onchange: () => changed++ });
    await source.create("environment", { name: "apiKey", value: "k3y-value", secret: true });
    const env = (await a.backend.environments()).find((e) => e.name === "Local")!;
    expect(env.variables.find((v) => v.name === "apiKey")).toEqual({
      name: "apiKey",
      value: null,
      enabled: true,
      secret: true,
    });
    expect(await a.backend.secrets.get("Local", "apiKey")).toBe("k3y-value");
    expect(JSON.stringify(source.cached())).not.toContain("k3y-value");
    expect(source.cached()!.variables.some((v) => v.name === "apiKey" && v.secret)).toBe(true);
    expect(a.reloads).toBe(1);
    expect(changed).toBe(1);
  });

  it("creates plain collection and folder variables unsaved", async () => {
    const a = await app();
    const source = createVarSource(a, { path: () => "users/list-users.yml", folder: () => "users" });
    await source.create("collection", { name: "tenant", value: "acme", secret: false });
    await source.create("folder", { name: "role", value: "admin", secret: false });
    expect(a.dirty).toEqual(expect.arrayContaining(["opencollection.yml", "users/folder.yml"]));
    const s = source.cached()!;
    expect(s.variables.find((v) => v.name === "tenant")?.source).toEqual({ layer: "collection" });
    expect(s.variables.find((v) => v.name === "role")?.source).toEqual({ layer: "folder", path: "users" });
    await expect(source.create("collection", { name: "x", value: "1", secret: true })).rejects.toThrow();
  });

  it("helpers", () => {
    expect(folderOf("users/admin/list.yml")).toBe("users/admin");
    expect(folderOf("health.yml")).toBeNull();
    const env = {
      name: "E",
      variables: [{ name: "a", value: "1", enabled: true, secret: false }],
      client_certificates: [],
      external_secrets: null,
    };
    expect(
      withEnvironmentVariable(env, { name: "a", value: "2", enabled: true, secret: false }).variables,
    ).toHaveLength(1);
    expect(
      withEnvironmentVariable(env, { name: "b", value: "2", enabled: true, secret: false }).variables,
    ).toHaveLength(2);
  });
});
