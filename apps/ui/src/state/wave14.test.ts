// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14 interface: the collection set as `AppState` exposes it (until strand A: the open
// collection).

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { AppState } from "./app.svelte";

describe("AppState collections (wave 14 interface)", () => {
  it("lists the open collection as the sidebar's only collection, with its tree", async () => {
    const app = new AppState(new MockBackend());
    expect(app.collections).toEqual([]);
    await app.open();
    const id = app.collection!.id;
    expect(app.collections.map((c) => [c.id, c.root, c.dir])).toEqual([[id, id, ""]]);
    expect(app.trees[id]).toEqual(app.tree);
    expect(app.activeCollection).toBe(id);
    await app.select("users/list-users.yml");
    expect(app.openTabs).toEqual([{ collection: id, path: "users/list-users.yml" }]);
  });

  it("selecting a request of the active collection by reference opens it", async () => {
    const app = new AppState(new MockBackend());
    await app.open();
    await app.selectRef({ collection: app.collection!.id, path: "users/create-user.yml" });
    expect(app.active).toBe("users/create-user.yml");
  });
});
