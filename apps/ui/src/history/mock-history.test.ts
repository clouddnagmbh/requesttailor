// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock's history and search behave like the core's for the UI tests.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { mockSearch } from "../commands/mock-history";

describe("mock history", () => {
  it("records sends newest first, filters and clears", async () => {
    const b = new MockBackend();
    await b.openCollection();
    await b.secrets.set("Local", "token", "s3cr3t-mock");
    await b.send("users/list-users.yml", { environment: "Local", vars: [] });
    await b.send("users/create-user.yml", { environment: "Local", vars: [] });
    await b.send("demo/blocked.yml", { environment: "Local", vars: [] });

    const all = await b.history.list("", 100);
    expect(all.map((e) => e.name)).toEqual(["Blocked by the browser", "Create user", "List users"]);
    expect(all[2]).toMatchObject({ method: "GET", status: 200, environment: "Local", collection: "mock-collection" });
    expect(all[0]).toMatchObject({ status: null, duration_ms: null });
    expect(JSON.stringify(all)).not.toContain("s3cr3t-mock");

    expect((await b.history.list("post", 100)).map((e) => e.name)).toEqual(["Create user"]);
    expect((await b.history.list("2xx user", 100)).map((e) => e.name)).toEqual(["Create user", "List users"]);
    expect((await b.history.list("error", 100)).map((e) => e.name)).toEqual(["Blocked by the browser"]);
    expect(await b.history.list("", 1)).toHaveLength(1);

    await b.history.clear();
    expect(await b.history.list("", 100)).toEqual([]);
  });
});

describe("mock search", () => {
  it("ranks name, URL, then the rest, with UTF-16 ranges", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const hits = await b.search("user", 50);
    expect(hits.map((h) => [h.path, h.field])).toEqual([
      ["users/list-users.yml", "name"],
      ["users/create-user.yml", "name"],
      ["users/list-users.yml", "url"],
      ["users/create-user.yml", "url"],
    ]);
    expect(hits[0].snippet).toBe("List users");
    expect(hits[0].ranges).toEqual([[5, 9]]);

    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, name: "🚀 Grüße Ärzte" });
    const emoji = await b.search("ärzte", 50);
    expect(emoji.map((h) => h.ranges)).toEqual([[[9, 14]]]);
    expect(mockSearch(new Map(), "x", 10)).toEqual([]);
    expect(await b.search("  ", 10)).toEqual([]);
  });
});
