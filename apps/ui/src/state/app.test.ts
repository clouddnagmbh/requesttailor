// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { AppState } from "./app.svelte";

describe("AppState", () => {
  it("sends the latest edit even when the backend answers out of order", async () => {
    const backend = new MockBackend();
    const update = backend.updateRequest.bind(backend);
    // Slow first update, as a concurrent shell could deliver it late.
    let calls = 0;
    backend.updateRequest = async (path, spec) => {
      if (calls++ === 0) await new Promise((r) => setTimeout(r, 30));
      return update(path, spec);
    };
    const app = new AppState(backend);
    await app.open();
    await app.select("health.yml");
    const spec = structuredClone(app.drafts["health.yml"]);
    void app.edit("health.yml", { ...spec, url: "{{baseUrl}}/one" });
    void app.edit("health.yml", { ...spec, url: "{{baseUrl}}/two" });
    await app.send("health.yml");
    expect(app.results["health.yml"].report.request?.url).toMatch(/\/two$/);
  });

  it("tells edit listeners about the user's edits, not about external ones (wave 10)", async () => {
    const backend = new MockBackend();
    const app = new AppState(backend);
    await app.open();
    await app.select("health.yml");
    const seen: string[] = [];
    const off = app.onEdit((path, spec) => seen.push(`${path} ${spec.url}`));
    const spec = structuredClone(app.drafts["health.yml"]);
    await app.edit("health.yml", { ...spec, url: "{{baseUrl}}/mine" });
    await app.applyExternal("health.yml", { ...spec, url: "{{baseUrl}}/theirs" });
    expect(seen).toEqual(["health.yml {{baseUrl}}/mine"]);
    // The open tab shows the external change, the backend has it, the request is unsaved.
    expect(app.drafts["health.yml"].url).toBe("{{baseUrl}}/theirs");
    expect((await backend.getRequest("health.yml")).url).toBe("{{baseUrl}}/theirs");
    expect(app.dirty).toContain("health.yml");
    off();
    await app.edit("health.yml", { ...spec, url: "{{baseUrl}}/after" });
    expect(seen).toHaveLength(1);
  });
});
