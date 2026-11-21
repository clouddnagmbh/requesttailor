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
});
