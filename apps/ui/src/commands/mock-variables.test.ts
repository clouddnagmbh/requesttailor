// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";

const local = { environment: "Local", vars: [] as [string, string][] };

describe("mock variables (VariablesApi)", () => {
  it("lists every layer in precedence order with overrides, like rt_app::variables", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const spec = await b.getRequest("users/list-users.yml");
    await b.updateRequest("users/list-users.yml", {
      ...spec,
      vars: [{ name: "userId", value: "7", enabled: true, secret: false }],
    });
    await b.scripts.setSessionVar("Local", { name: "session", value: "s1", scope: "runtime" });
    const s = await b.variables.scope("users/list-users.yml", local);
    expect(s.variables.map((v) => `${v.name}@${v.source.layer}${v.overridden ? "*" : ""}`)).toEqual([
      "apiVersion@collection",
      "baseUrl@environment",
      "token@environment",
      "userId@folder*",
      "userId@request",
      "session@runtime",
    ]);
    expect(s.variables.find((v) => v.name === "token")).toMatchObject({ secret: true, value: null });
    // Folders and the collection see only their layers.
    const root = await b.variables.scope("", local);
    expect(root.variables.map((v) => v.name)).toEqual(["apiVersion", "baseUrl", "token", "session"]);
    await expect(b.variables.scope("nope.yml", local)).rejects.toThrow();
  });

  it("never returns a secret value, not even copied into a runtime value", async () => {
    const b = new MockBackend();
    await b.openCollection();
    await b.secrets.set("Local", "token", "t0p-s3cret");
    await b.scripts.setSessionVar("Local", { name: "auth", value: "Bearer t0p-s3cret", scope: "runtime" });
    const s = await b.variables.scope("health.yml", local);
    expect(JSON.stringify(s)).not.toContain("t0p-s3cret");
    expect(s.variables.find((v) => v.name === "auth")?.value).toBe("Bearer ***");
  });

  it("defines collection and folder variables unsaved; the send resolves them", async () => {
    const b = new MockBackend();
    await b.openCollection();
    await b.variables.define("", { name: "tenant", value: "acme", enabled: true, secret: false });
    await b.variables.define("users", { name: "userId", value: "7", enabled: true, secret: false });
    expect(await b.dirty()).toEqual(expect.arrayContaining(["opencollection.yml", "users/folder.yml"]));
    await expect(b.variables.define("", { name: "k", value: null, enabled: true, secret: true })).rejects.toThrow(/environment/);
    await expect(b.variables.define("", { name: "a}b", value: "1", enabled: true, secret: false })).rejects.toThrow();
    await expect(b.variables.define("nope", { name: "a", value: "1", enabled: true, secret: false })).rejects.toThrow();

    const spec = await b.getRequest("users/list-users.yml");
    await b.updateRequest("users/list-users.yml", { ...spec, url: "{{baseUrl}}/{{tenant}}/users/{{userId}}" });
    const preview = await b.preview("users/list-users.yml", local);
    expect(preview.url).toBe("http://127.0.0.1:47811/acme/users/7");
    expect(preview.variables).toContainEqual({ name: "tenant", value: "acme", source: { layer: "collection" }, secret: false });
    expect(preview.unresolved).toEqual([]);

    const saved = await b.save();
    expect(saved.map((c) => c.path)).toEqual(expect.arrayContaining(["opencollection.yml", "users/folder.yml"]));
    expect(await b.dirty()).not.toContain("opencollection.yml");
  });
});
