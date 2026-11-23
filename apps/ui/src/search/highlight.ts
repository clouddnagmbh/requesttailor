// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the search view: highlighted snippet parts and hits grouped by request.

import type { SearchField, SearchHit } from "../commands/types";

export interface Segment {
  text: string;
  mark: boolean;
}

/** `snippet` cut at `ranges` (UTF-16 offsets, which is how JavaScript indexes strings).
 *  Ranges out of order, overlapping or out of bounds are tolerated. */
export function segments(snippet: string, ranges: readonly [number, number][]): Segment[] {
  const out: Segment[] = [];
  let at = 0;
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  for (const [s0, e0] of sorted) {
    const s = Math.max(at, Math.min(s0, snippet.length));
    const e = Math.max(s, Math.min(e0, snippet.length));
    if (e === s) continue;
    if (s > at) out.push({ text: snippet.slice(at, s), mark: false });
    out.push({ text: snippet.slice(s, e), mark: true });
    at = e;
  }
  if (at < snippet.length) out.push({ text: snippet.slice(at), mark: false });
  return out;
}

export interface HitGroup<H extends SearchHit = SearchHit> {
  /** Wave 14: the collection of the request (hits of `collections.search`). */
  collection?: string;
  path: string;
  name: string;
  method: string;
  /** Hits with their position in display order (for keyboard navigation). */
  hits: { hit: H; index: number }[];
}

/** Hits grouped by request, groups in the order of their first (best) hit. `index` counts
 *  hits in display order: group by group. */
export function groupHits<H extends SearchHit & { collection?: string }>(hits: readonly H[]): HitGroup<H>[] {
  const groups = new Map<string, HitGroup<H>>();
  for (const hit of hits) {
    const key = `${hit.collection ?? ""}\u0000${hit.path}`;
    let g = groups.get(key);
    if (!g) {
      g = { path: hit.path, name: hit.name, method: hit.method, hits: [] };
      if (hit.collection !== undefined) g.collection = hit.collection;
      groups.set(key, g);
    }
    g.hits.push({ hit, index: -1 });
  }
  let index = 0;
  for (const g of groups.values()) for (const h of g.hits) h.index = index++;
  return [...groups.values()];
}

export const FIELD_LABELS: Record<SearchField, string> = {
  name: "Name",
  url: "URL",
  param: "Param",
  header: "Header",
  body: "Body",
  auth: "Auth",
  variable: "Variable",
};
