// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The sidebar's rows over several collections (wave 14, strand A).

import { describe, expect, it } from "vitest";

import type { CollectionEntry, TreeNode } from "../commands/types";
import { collectionKey, folderKey, repoStatusParts, sideKey, sideRows, typeahead } from "./tree";

const entry = (id: string, name: string): CollectionEntry => ({ id, name, location: id, root: "r", dir: "", repo: null, dirty: 0 });
const tree = (name: string): TreeNode => ({
  kind: "folder",
  name,
  path: "",
  children: [
    {
      kind: "folder",
      name: "users",
      path: "users",
      children: [{ kind: "request", name: "List users", path: "users/list.yml", method: "GET" }],
    },
    { kind: "request", name: "Health", path: "health.yml", method: "GET" },
  ],
});

describe("sideRows", () => {
  const collections = [entry("a", "Alpha"), entry("b", "Beta")];

  it("lists every collection; an expanded one shows its rows one level deeper", () => {
    const rows = sideRows(collections, { a: tree("Alpha") }, new Set(["a"]), new Set());
    expect(rows.map((r) => [r.node.name, r.level, r.collection])).toEqual([
      ["Alpha", 1, "a"],
      ["users", 2, "a"],
      ["List users", 3, "a"],
      ["Health", 2, "a"],
      ["Beta", 1, "b"],
    ]);
    expect(rows[1].parent).toBe(collectionKey("a"));
    expect(rows[2].parent).toBe(rows[1].key);
    expect(rows.map((r) => [r.posinset, r.setsize])).toEqual([
      [1, 2],
      [1, 2],
      [1, 1],
      [2, 2],
      [2, 2],
    ]);
  });

  it("keeps the same path in two collections apart and folds folders per collection", () => {
    const rows = sideRows(collections, { a: tree("Alpha"), b: tree("Beta") }, new Set(["a", "b"]), new Set([folderKey("b", "users")]));
    const health = rows.filter((r) => r.node.path === "health.yml");
    expect(health.map((r) => r.key)).toEqual([
      sideKey("a", { kind: "request", path: "health.yml", name: "", method: "" }),
      sideKey("b", { kind: "request", path: "health.yml", name: "", method: "" }),
    ]);
    expect(rows.filter((r) => r.node.name === "List users").map((r) => r.collection)).toEqual(["a"]);
  });

  it("marks an expanded collection whose tree is not read yet", () => {
    const rows = sideRows(collections, { a: tree("Alpha") }, new Set(["a", "b"]), new Set());
    expect(rows.at(-1)).toMatchObject({ node: { name: "Beta" }, loading: true });
    expect(rows[0].loading).toBe(false);
  });

  it("type-ahead walks collection and request rows alike", () => {
    const rows = sideRows(collections, { a: tree("Alpha") }, new Set(["a"]), new Set());
    expect(typeahead(rows, 0, "h")).toBe(3);
    expect(typeahead(rows, 3, "b")).toBe(4);
  });
});

describe("repository status (wave 15)", () => {
  const base = { id: "/r", branch: "main", upstream: "origin/main", ahead: 0, behind: 0, changes: 0 };

  it("shows branch, sync, changes and a half-done operation", () => {
    expect(repoStatusParts(null)).toBeNull();
    expect(repoStatusParts({ ...base })).toMatchObject({ branch: "main", sync: "", changes: 0, text: "Branch main, no changes" });
    const busy = repoStatusParts({ ...base, ahead: 2, behind: 1, changes: 3, operation: "merge" })!;
    expect(busy.sync).toBe("↑2 ↓1");
    expect(busy.operation).toBe("Merging");
    expect(busy.text).toBe("Branch main, 2 ahead, 1 behind, 3 changed files, Merging");
    expect(repoStatusParts({ ...base, branch: null, changes: 1 })!.text).toBe("Branch detached, 1 changed file");
  });
});
