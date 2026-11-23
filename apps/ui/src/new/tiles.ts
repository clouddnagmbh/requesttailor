// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The tiles of the "New…" dialog (wave 14, strand B): the built-in kinds (./kinds.ts) and the
// tiles of extensions (./items.ts) in one list, grouped, filtered as the user types, and the
// arrow-key moves between them on the grid as it is laid out.

import { fuzzyScore } from "../shell/fuzzy";
import type { ExtensionNewItemRef } from "./items";
import { NEW_ITEMS, type NewItemKind } from "./kinds";
import { aiFor } from "../ai/ai.svelte";
import { openOpenApiWizard } from "../openapi/store.svelte";
import CloneRepositoryDialog from "../git/CloneRepositoryDialog.svelte";

/** "From OpenAPI": opens the OpenAPI import wizard (it asks for the target itself). */
const OPENAPI_TILE: Tile = {
  key: "openapi",
  title: "From OpenAPI",
  description: "Requests and environments from an OpenAPI or Swagger spec.",
  group: "Organise",
  icon: "import",
  extension: {
    key: "openapi",
    extension: "core",
    item: { id: "openapi", title: "From OpenAPI", description: "", run: () => openOpenApiWizard() },
  },
};

/** "Clone repository" (wave 15): any Git repository into a new folder, added to the sidebar. */
const CLONE_TILE: Tile = {
  key: "clone-repository",
  title: "Clone repository",
  description: "Any Git repository from an address, into a new folder in the sidebar.",
  group: "Organise",
  icon: "import",
  extension: {
    key: "clone-repository",
    extension: "core",
    item: {
      id: "clone-repository",
      title: "Clone repository",
      description: "",
      run: (ctx) => void ctx.app.showDialog({ title: "Clone a repository", component: CloneRepositoryDialog, width: 520 }),
    },
  },
};

/** "Open repository" (wave 15): a folder in Git joins the sidebar, with or without a collection. */
const OPEN_REPOSITORY_TILE: Tile = {
  key: "open-repository",
  title: "Open repository",
  description: "A folder on this computer that is a Git repository, with or without a collection.",
  group: "Organise",
  icon: "folder",
  extension: {
    key: "open-repository",
    extension: "core",
    item: { id: "open-repository", title: "Open repository", description: "", run: (ctx) => ctx.app.openRepository() },
  },
};

/** "From description or snippet": a request from a sentence, or from pasted cURL, code or API
 *  documentation text. Only offered once the assistant is set up and allowed here (the cURL
 *  part works without a model, but the tile is an assistant entry). */
const AI_TILE: Tile = {
  key: "ai-request",
  title: "From description or snippet",
  description: "Describe a request, or paste cURL, code or API docs, and review the proposal.",
  group: "Requests",
  icon: "spark",
  extension: {
    key: "ai-request",
    extension: "core",
    item: {
      id: "ai-request",
      title: "From description or snippet",
      description: "",
      run: async (ctx, target) => {
        if (target.collection && target.collection !== ctx.app.activeCollection) await ctx.app.activate(target.collection);
        aiFor(ctx.backend).open({ kind: "create", folder: target.folder });
      },
    },
  },
};

export interface Tile {
  /** The built-in kind, or `<extension id>/<item id>`. */
  key: string;
  title: string;
  description: string;
  group: string;
  icon: string;
  /** Extension tiles only (at most 12 characters). */
  badge?: string;
  builtin?: NewItemKind;
  extension?: ExtensionNewItemRef;
  /** Why the tile cannot be chosen right now, or absent. */
  disabled?: string;
}

/** More words a built-in tile is found by. */
const KEYWORDS: Record<NewItemKind, string> = {
  http: "rest api get post put url",
  graphql: "query mutation schema",
  grpc: "protobuf proto rpc",
  websocket: "ws socket realtime",
  sse: "sse event stream server sent",
  collection: "project api",
  folder: "group directory",
  environment: "env variables stage",
  "mcp-test": "mcp model context protocol tool agent",
};

export const BUILTIN_GROUPS = ["Requests", "Organise", "Testing"] as const;
export const MORE_GROUP = "More";

/** Every tile: the built-in ones first, then the extensions' in their order. */
export function allTiles(extensions: readonly ExtensionNewItemRef[], hasCollection: boolean, aiReady = false): Tile[] {
  const builtin = NEW_ITEMS.map(
    (i): Tile => ({
      key: i.kind,
      title: i.title,
      description: i.description,
      group: i.group,
      icon: i.icon,
      builtin: i.kind,
      ...(i.needsCollection && !hasCollection ? { disabled: "Open or create a collection first." } : {}),
    }),
  );
  const more = extensions.map(
    (ref): Tile => ({
      key: ref.key,
      title: ref.item.title,
      description: ref.item.description,
      group: ref.item.group?.trim() || MORE_GROUP,
      icon: ref.item.icon ?? "",
      ...(ref.item.badge ? { badge: ref.item.badge.slice(0, 12) } : {}),
      extension: ref,
    }),
  );
  const ai: Tile[] = aiReady ? [hasCollection ? AI_TILE : { ...AI_TILE, disabled: "Open or create a collection first." }] : [];
  return [...builtin, OPENAPI_TILE, CLONE_TILE, OPEN_REPOSITORY_TILE, ...ai, ...more];
}

export interface TileGroup {
  group: string;
  tiles: Tile[];
}

/** `tiles` under their headings: built-in groups first, then the others in the order they
 *  first occur; empty groups are left out. */
export function groupTiles(tiles: readonly Tile[]): TileGroup[] {
  const order: string[] = [...BUILTIN_GROUPS];
  for (const t of tiles) if (!order.includes(t.group)) order.push(t.group);
  return order.map((group) => ({ group, tiles: tiles.filter((t) => t.group === group) })).filter((g) => g.tiles.length > 0);
}

function score(tile: Tile, query: string): number | null {
  const words = tile.builtin ? KEYWORDS[tile.builtin] : "";
  const title = fuzzyScore(query, tile.title);
  const more = fuzzyScore(query, `${tile.group} ${words}`);
  if (title === null) return more;
  // A match in the title counts a little more than one in the extra words.
  return Math.max(title + 1, more ?? -Infinity);
}

export interface Filtered {
  /** The matching tiles in display order (grouped). */
  groups: TileGroup[];
  /** The tiles in display order, flat; index into it for the keyboard. */
  flat: Tile[];
  /** Index in `flat` of the best match (`0` without a query). */
  best: number;
}

/** The tiles matching `query`, grouped as shown, and which one Enter takes. */
export function filterTiles(tiles: readonly Tile[], query: string): Filtered {
  const q = query.trim();
  const scored = tiles.map((t) => ({ t, s: q === "" ? 0 : score(t, q) })).filter((x) => x.s !== null);
  const groups = groupTiles(scored.map((x) => x.t));
  const flat = groups.flatMap((g) => g.tiles);
  let best = flat.findIndex((t) => !t.disabled);
  if (q !== "") {
    let top = -Infinity;
    for (const { t, s } of scored) {
      const i = flat.indexOf(t);
      if (!t.disabled && s! > top) {
        top = s!;
        best = i;
      }
    }
  }
  return { groups, flat, best: Math.max(0, best) };
}

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export type MoveKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End";

/** The tile an arrow key moves to from `current`, on the grid as laid out (`boxes` in display
 *  order): Left/Right step through the order, Up/Down go to the nearest tile in the row above
 *  or below (across group headings), Home/End to the first and last. Stays at the edges. */
export function moveTile(boxes: readonly Box[], current: number, key: MoveKey): number {
  const n = boxes.length;
  if (n === 0) return 0;
  const at = Math.min(Math.max(current, 0), n - 1);
  switch (key) {
    case "Home":
      return 0;
    case "End":
      return n - 1;
    case "ArrowLeft":
      return Math.max(0, at - 1);
    case "ArrowRight":
      return Math.min(n - 1, at + 1);
  }
  const from = boxes[at];
  const cx = from.left + from.width / 2;
  const cy = from.top + from.height / 2;
  const down = key === "ArrowDown";
  let pick = at;
  let bestDy = Infinity;
  let bestDx = Infinity;
  boxes.forEach((b, i) => {
    const dy = b.top + b.height / 2 - cy;
    if (down ? dy <= from.height / 2 : dy >= -from.height / 2) return;
    const ady = Math.abs(dy);
    const adx = Math.abs(b.left + b.width / 2 - cx);
    // The nearest row first (a row is everything within half a tile), then the nearest column.
    if (ady < bestDy - from.height / 2 || (Math.abs(ady - bestDy) <= from.height / 2 && adx < bestDx)) {
      bestDy = ady;
      bestDx = adx;
      pick = i;
    }
  });
  return pick;
}
