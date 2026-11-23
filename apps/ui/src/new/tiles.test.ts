// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand B: tiles of the "New…" dialog (groups, filter, arrow keys on the grid).

import { describe, expect, it } from "vitest";

import type { ExtensionNewItemRef } from "./items";
import { allTiles, filterTiles, groupTiles, moveTile, type Box } from "./tiles";

const ext = (key: string, over: Partial<ExtensionNewItemRef["item"]> = {}): ExtensionNewItemRef => ({
  key,
  extension: key.split("/")[0],
  item: { id: key.split("/")[1], title: key, description: "From an extension.", run: () => {}, ...over },
});

describe("tiles", () => {
  it("built-in groups first, then extension groups in order, ungrouped under More", () => {
    const tiles = allTiles(
      [ext("x/one", { group: "Data", badge: "A very long badge" }), ext("x/two"), ext("y/three", { group: "Data" })],
      true,
    );
    expect(groupTiles(tiles).map((g) => [g.group, g.tiles.length])).toEqual([
      ["Requests", 5],
      ["Organise", 3],
      ["Testing", 1],
      ["Data", 2],
      ["More", 1],
    ]);
    expect(tiles.find((t) => t.key === "x/one")?.badge).toBe("A very long ");
    // Built-in tiles never carry a badge.
    expect(tiles.filter((t) => t.builtin).every((t) => t.badge === undefined)).toBe(true);
  });

  it("without a collection only Collection and extension tiles can be chosen", () => {
    const tiles = allTiles([ext("x/one")], false);
    expect(tiles.filter((t) => !t.disabled).map((t) => t.key)).toEqual(["collection", "x/one"]);
    expect(filterTiles(tiles, "").best).toBe(tiles.findIndex((t) => t.key === "collection"));
  });

  it("typing filters and picks the best match", () => {
    const tiles = allTiles([ext("x/odata", { title: "OData service" })], true);
    const pick = (q: string) => {
      const f = filterTiles(tiles, q);
      return f.flat[f.best]?.key;
    };
    expect(pick("")).toBe("http");
    expect(pick("graph")).toBe("graphql");
    expect(pick("ws")).toBe("websocket");
    expect(pick("sse")).toBe("sse");
    expect(pick("env")).toBe("environment");
    expect(pick("fold")).toBe("folder");
    expect(pick("mcp")).toBe("mcp-test");
    expect(pick("proto")).toBe("grpc");
    expect(pick("odata")).toBe("x/odata");
    expect(filterTiles(tiles, "zzzz").flat).toEqual([]);
  });
});

describe("arrow keys on the grid", () => {
  // Two groups: three tiles in a row of three, then two tiles in the next group's row.
  const box = (col: number, row: number): Box => ({ left: col * 110, top: row * 90, width: 100, height: 80 });
  const boxes = [box(0, 0), box(1, 0), box(2, 0), box(0, 1.4), box(1, 1.4)];

  it("Left and Right follow the order, Home and End jump", () => {
    expect(moveTile(boxes, 0, "ArrowRight")).toBe(1);
    expect(moveTile(boxes, 2, "ArrowRight")).toBe(3);
    expect(moveTile(boxes, 4, "ArrowRight")).toBe(4);
    expect(moveTile(boxes, 0, "ArrowLeft")).toBe(0);
    expect(moveTile(boxes, 3, "End")).toBe(4);
    expect(moveTile(boxes, 3, "Home")).toBe(0);
  });

  it("Up and Down go to the nearest tile in the next row, across headings", () => {
    expect(moveTile(boxes, 1, "ArrowDown")).toBe(4);
    expect(moveTile(boxes, 2, "ArrowDown")).toBe(4);
    expect(moveTile(boxes, 0, "ArrowDown")).toBe(3);
    expect(moveTile(boxes, 4, "ArrowUp")).toBe(1);
    expect(moveTile(boxes, 3, "ArrowDown")).toBe(3);
    expect(moveTile(boxes, 1, "ArrowUp")).toBe(1);
    expect(moveTile([], 0, "ArrowDown")).toBe(0);
  });
});
