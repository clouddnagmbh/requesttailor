// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { repositoriesOfCollections, stubCreatePull, stubRepositories, withRepositories } from "./wave15-stubs";
import type { CollectionEntry } from "./types";

const entry = (id: string, repo: string | null): CollectionEntry => ({ id, name: id, location: id, root: "/work", dir: id, repo, dirty: 0 });

describe("wave 15 interface placeholders", () => {
  it("reject with kind unsupported until a strand lands", async () => {
    await expect(stubRepositories().clone({ url: "https://example.com/r.git" })).rejects.toMatchObject({ kind: "unsupported" });
    await expect(stubCreatePull({ title: "T", body: "", head: "a", base: "main", draft: false })).rejects.toMatchObject({ kind: "unsupported" });
    const b = new MockBackend();
    await expect(b.git.workingCopy()).rejects.toMatchObject({ kind: "unsupported" });
    await expect(b.git.blame("HEAD", "/a")).rejects.toMatchObject({ kind: "unsupported" });
    await expect(b.git.stage(["/a"])).rejects.toMatchObject({ kind: "unsupported" });
    await expect(b.repositories.init()).rejects.toMatchObject({ kind: "unsupported" });
  });

  it("derives the repositories of a set from its collections", () => {
    const collections = [entry("/work/a", "/work"), entry("/work/b", "/work"), entry("/scratch", null), entry("/other/c", "/other")];
    expect(repositoriesOfCollections(collections).map((r) => [r.id, r.name, r.collections])).toEqual([
      ["/work", "work", ["/work/a", "/work/b"]],
      ["/other", "other", ["/other/c"]],
    ]);
    expect(withRepositories({ roots: [], collections, active: "/other/c" }).active_repository).toBe("/other");
    expect(withRepositories({ roots: [], collections, active: "/scratch" }).active_repository).toBeUndefined();
  });
});
