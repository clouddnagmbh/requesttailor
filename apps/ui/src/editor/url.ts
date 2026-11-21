// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// URL <-> param rows. The URL is the truth (as in Bruno and in rt-core's `prepare`): query
// rows of kind "query" only mirror the URL's query string, path rows of kind "path" carry the
// values for `:name` segments. Nothing here decodes or re-encodes what the user wrote:
// pairs that did not change keep their exact text, `{{templates}}` are opaque.

import type { Param } from "../commands/types";

/** `url === base + ("?" + query if query !== null) + fragment` */
export interface UrlParts {
  base: string;
  /** Text after the first `?`, `null` without `?`. */
  query: string | null;
  /** `#...` after the query, as written; `""` if none. */
  fragment: string;
}

export interface QueryPair {
  name: string;
  value: string;
  /** The pair as written in the URL. */
  raw: string;
}

/** Placeholder ranges `[start, end)`, same matches as rt-core (`\{\{([^}]+)\}\}`). */
function templateRanges(text: string): [number, number][] {
  const out: [number, number][] = [];
  const re = /\{\{([^}]+)\}\}/g;
  for (let m = re.exec(text); m; m = re.exec(text)) out.push([m.index, m.index + m[0].length]);
  return out;
}

/** Indexes of `chars` in `text` that are not inside a `{{placeholder}}`. */
function indexesOutside(text: string, chars: string): number[] {
  const ranges = templateRanges(text);
  const out: number[] = [];
  let r = 0;
  for (let i = 0; i < text.length; i++) {
    while (r < ranges.length && ranges[r][1] <= i) r++;
    if (r < ranges.length && ranges[r][0] <= i) {
      i = ranges[r][1] - 1;
      continue;
    }
    if (chars.includes(text[i])) out.push(i);
  }
  return out;
}

function splitOutside(text: string, sep: string): string[] {
  const parts: string[] = [];
  let start = 0;
  for (const i of indexesOutside(text, sep)) {
    parts.push(text.slice(start, i));
    start = i + 1;
  }
  parts.push(text.slice(start));
  return parts;
}

export function splitUrl(url: string): UrlParts {
  const q = indexesOutside(url, "?")[0];
  if (q === undefined) return { base: url, query: null, fragment: "" };
  const rest = url.slice(q + 1);
  const h = indexesOutside(rest, "#")[0];
  return {
    base: url.slice(0, q),
    query: h === undefined ? rest : rest.slice(0, h),
    fragment: h === undefined ? "" : rest.slice(h),
  };
}

export function joinUrl(parts: UrlParts): string {
  return parts.base + (parts.query === null ? "" : "?" + parts.query) + parts.fragment;
}

/** Pairs of a query string, in order, repeated names kept, empty pieces (`&&`) skipped. */
export function parseQuery(query: string): QueryPair[] {
  return splitOutside(query, "&")
    .filter((raw) => raw !== "")
    .map((raw) => {
      const eq = indexesOutside(raw, "=")[0];
      return eq === undefined
        ? { name: raw, value: "", raw }
        : { name: raw.slice(0, eq), value: raw.slice(eq + 1), raw };
    });
}

/** Names of path params in the URL, first occurrence first: `:name` segments and `:name`
 *  inside OData-style segments such as `Users(:id)` (same rules as rt-core). */
export function pathParamNames(url: string): string[] {
  let base = splitUrl(url).base;
  const scheme = indexesOutside(base, ":").find((i) => base.startsWith("://", i));
  if (scheme !== undefined) {
    // Skip `scheme://authority`.
    const afterAuthority = base.slice(scheme + 3);
    const slash = indexesOutside(afterAuthority, "/")[0];
    base = slash === undefined ? "" : afterAuthority.slice(slash);
  }
  const names: string[] = [];
  const add = (n: string) => {
    if (n && !names.includes(n)) names.push(n);
  };
  for (const segment of splitOutside(base, "/")) {
    if (segment.startsWith(":")) add(segment.slice(1));
    else if (/^[A-Za-z0-9_.-]+\([^)]*\)$/.test(segment)) {
      for (const m of segment.matchAll(/:([A-Za-z_]\w*)/g)) add(m[1]);
    }
  }
  return names;
}

const isBlank = (p: Param) => p.name === "" && p.value === "";

/**
 * The URL was edited: query rows follow the URL's query string, disabled query rows stay where
 * they were, path rows follow the URL's `:name`s and keep their values by name.
 */
export function paramsFromUrl(url: string, old: Param[]): Param[] {
  const { query } = splitUrl(url);
  const pairs = query === null ? [] : parseQuery(query);
  const rows: Param[] = [];
  let k = 0;
  for (const row of old) {
    if (row.kind !== "query") continue;
    if (!row.enabled) rows.push(row);
    else if (!isBlank(row) && k < pairs.length) {
      rows.push({ name: pairs[k].name, value: pairs[k].value, kind: "query", enabled: true });
      k++;
    }
  }
  for (; k < pairs.length; k++) rows.push({ name: pairs[k].name, value: pairs[k].value, kind: "query", enabled: true });

  const oldPath = old.filter((r) => r.kind === "path");
  for (const name of pathParamNames(url)) {
    rows.push(oldPath.find((r) => r.name === name) ?? { name, value: "", kind: "path", enabled: true });
  }
  return rows;
}

/** Percent-escapes characters that would change the query's structure, outside `{{ }}`. */
function escapePart(text: string, chars: string): string {
  const at = new Set(indexesOutside(text, chars));
  let out = "";
  for (let i = 0; i < text.length; i++) {
    out += at.has(i) ? "%" + text.charCodeAt(i).toString(16).toUpperCase().padStart(2, "0") : text[i];
  }
  return out;
}

/**
 * Query rows were edited: rebuilds the URL's query string from the enabled query rows.
 * Pairs that match a pair already in the URL keep their exact text; blank rows are skipped;
 * with no enabled rows the `?` goes away. Base and fragment are unchanged.
 */
export function urlFromParams(url: string, params: Param[]): string {
  const parts = splitUrl(url);
  const pool = parts.query === null ? [] : parseQuery(parts.query);
  const used = new Array<boolean>(pool.length).fill(false);
  const pieces: string[] = [];
  for (const row of params) {
    if (row.kind !== "query" || !row.enabled || isBlank(row)) continue;
    const i = pool.findIndex((pair, j) => !used[j] && pair.name === row.name && pair.value === row.value);
    if (i >= 0) {
      used[i] = true;
      pieces.push(pool[i].raw);
    } else {
      pieces.push(`${escapePart(row.name, "&#=")}=${escapePart(row.value, "&#")}`);
    }
  }
  return joinUrl({ ...parts, query: pieces.length ? pieces.join("&") : null });
}
