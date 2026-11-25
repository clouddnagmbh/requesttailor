// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { mockConsumers } from "./mock-consumers";

describe("mock consumers", () => {
  it("lists three and names the breaking consumer and request", async () => {
    const api = mockConsumers();
    expect((await api.list()).map((c) => c.name)).toEqual(["billing", "portal", "mobile"]);
    const green = await api.verify({ base_url: "http://localhost:9" });
    expect(green.consumers.every((c) => c.failures.length === 0)).toBe(true);
    const broken = await api.verify({ base_url: "http://break.test" });
    expect(broken.consumers[0]!.failures[0]!.request).toBe("GET /orders/{orderId}");
    expect(broken.consumers[1]!.failures).toEqual([]);
  });

  it("leaves writes out without allow_writes and refuses an unknown name", async () => {
    const api = mockConsumers();
    const without = await api.verify({ base_url: "http://x", only: ["billing"] });
    expect(without.consumers).toHaveLength(1);
    expect(without.consumers[0]).toMatchObject({ requests: 1, skipped: 1 });
    const withWrites = await api.verify({ base_url: "http://x", only: ["billing"], allow_writes: true });
    expect(withWrites.consumers[0]).toMatchObject({ requests: 2 });
    expect(withWrites.consumers[0]!.skipped).toBeUndefined();
    await expect(api.verify({ base_url: "http://x", only: ["nobody"] })).rejects.toThrow(/nobody/);
  });
});
