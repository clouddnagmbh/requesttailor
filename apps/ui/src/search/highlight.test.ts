// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { SearchHit } from "../commands/types";
import { groupHits, segments } from "./highlight";

const hit = (path: string, snippet = "x"): SearchHit => ({
  path,
  name: path,
  method: "GET",
  field: "name",
  snippet,
  ranges: [],
});

describe("search highlight", () => {
  it("cuts the snippet at UTF-16 ranges", () => {
    expect(segments("Get user", [[4, 8]])).toEqual([
      { text: "Get ", mark: false },
      { text: "user", mark: true },
    ]);
    // 🚀 is two UTF-16 units, as the core counts them.
    expect(
      segments("🚀 Ärzte ÄRZTE", [
        [3, 8],
        [9, 14],
      ]),
    ).toEqual([
      { text: "🚀 ", mark: false },
      { text: "Ärzte", mark: true },
      { text: " ", mark: false },
      { text: "ÄRZTE", mark: true },
    ]);
    expect(segments("abc", [])).toEqual([{ text: "abc", mark: false }]);
    // Unsorted, overlapping, out of bounds.
    expect(
      segments("abcdef", [
        [4, 99],
        [0, 2],
        [1, 3],
      ]),
    ).toEqual([
      { text: "ab", mark: true },
      { text: "c", mark: true },
      { text: "d", mark: false },
      { text: "ef", mark: true },
    ]);
  });

  it("groups hits by request in order of the first hit", () => {
    const groups = groupHits([hit("b.yml", "1"), hit("a.yml", "2"), hit("b.yml", "3")]);
    // Indexes count in display order, group by group.
    expect(groups.map((g) => [g.path, g.hits.map((h) => [h.index, h.hit.snippet])])).toEqual([
      ["b.yml", [[0, "1"], [1, "3"]]],
      ["a.yml", [[2, "2"]]],
    ]);
  });
});
