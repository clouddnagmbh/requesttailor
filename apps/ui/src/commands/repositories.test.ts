// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The TypeScript twin of crates/rt-app/tests/repositories.rs: the rules web and mock share.

import { describe, expect, it } from "vitest";

import { activeRepositoryOf, repositoriesOf, repositoryName, withRepositories } from "./repositories";
import type { CollectionEntry } from "./types";

const entry = (id: string, root: string, repo: string | null): CollectionEntry => ({ id, name: id, location: id, root, dir: id, repo, dirty: 0 });

describe("repositories of a set", () => {
  const collections = [
    entry("/work/a", "/work", "/work"),
    entry("/work/b", "/work", "/work"),
    entry("/scratch", "/scratch", null),
    entry("/other/c", "/other", "/other"),
  ];

  it("lists repositories with collections first, then repository roots", () => {
    const list = repositoriesOf(collections, [
      { root: "/notes", repo: "/notes" },
      { root: "/work", repo: "/work" },
    ]);
    expect(list.map((r) => [r.id, r.name, r.root, r.collections.length])).toEqual([
      ["/work", "work", "/work", 2],
      ["/other", "other", "/other", 1],
      ["/notes", "notes", "/notes", 0],
    ]);
  });

  it("lists a repository reached from two roots once, under the first", () => {
    const list = repositoriesOf([entry("/w/a", "/w", "/w")], [
      { root: "/w/sub", repo: "/w" },
      { root: "/v", repo: "/v" },
      { root: "/v/also", repo: "/v" },
    ]);
    expect(list.map((r) => [r.id, r.root])).toEqual([
      ["/w", "/w"],
      ["/v", "/v"],
    ]);
  });

  it("keeps nested repositories apart", () => {
    const list = repositoriesOf([entry("/outer/inner", "/outer", "/outer/inner")], [{ root: "/outer", repo: "/outer" }]);
    expect(list.map((r) => r.id)).toEqual(["/outer/inner", "/outer"]);
    expect(list[1].collections).toEqual([]);
  });

  it("follows the sidebar choice, then the active collection", () => {
    const roots = [{ root: "/notes", repo: "/notes" }];
    const list = repositoriesOf(collections, roots);
    expect(activeRepositoryOf(list, collections, null, "/work/a")).toBe("/work");
    expect(activeRepositoryOf(list, collections, null, "/scratch")).toBeUndefined();
    expect(activeRepositoryOf(list, collections, null, null)).toBeUndefined();
    expect(activeRepositoryOf(list, collections, "/notes", "/work/a")).toBe("/notes");
    expect(activeRepositoryOf(list, collections, "/notes", null)).toBe("/notes");
    expect(activeRepositoryOf(list, collections, "/gone", "/work/a")).toBe("/work");
  });

  it("puts both on a set", () => {
    const set = withRepositories({ roots: [], collections, active: "/other/c" });
    expect(set.active_repository).toBe("/other");
    expect(set.repositories.map((r) => r.id)).toEqual(["/work", "/other"]);
    expect(withRepositories({ roots: [], collections, active: "/scratch" }).active_repository).toBeUndefined();
    expect(withRepositories({ roots: [], collections, active: null }, { repoRoots: [{ root: "/n", repo: "/n" }], chosen: "/n" }).active_repository).toBe("/n");
  });

  it("names a repository by its last segment", () => {
    expect(repositoryName("/work/api/")).toBe("api");
    expect(repositoryName("C:\\work\\api")).toBe("api");
  });
});
