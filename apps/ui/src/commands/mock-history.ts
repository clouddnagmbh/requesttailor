// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// History and search in the mock backend, for their UI tests. Owner: wave 4, strand E.
// They follow the real ones (`rt_app::history`, `rt_app::search`) closely enough for the UI:
// same filter words, same ranking, same snippet ranges (JavaScript strings are UTF-16, like
// the core's ranges). Tests may add entries with `record` directly
// (`window.__rt.backend.history`).

import type { HistoryApi } from "./backend";
import type { HistoryEntry, RequestSpec, SearchField, SearchHit, StoredResponse } from "./types";

export const MOCK_COLLECTION = "mock-collection";
const HISTORY_LIMIT = 5_000;
const SEARCH_LIMIT = 200;
const SNIPPET_CHARS = 80;
const CONTEXT_BEFORE = 24;

function errorText(entry: HistoryEntry): string {
  const e = entry.error;
  if (!e) return "";
  if (e.kind === "transport") return `transport ${e.message.kind} ${"message" in e.message ? e.message.message : ""}`;
  return `${e.kind} ${e.message}`;
}

/** `rt_app::history` word matching. */
function matchesWord(e: HistoryEntry, word: string): boolean {
  const has = (t: string | null) => t != null && t.toLowerCase().includes(word);
  if (has(e.name) || has(e.method) || has(e.request_path) || has(e.url) || has(e.environment)) return true;
  if (e.status != null) {
    const status = String(e.status);
    if (/^[0-9]xx$/.test(word) && status[0] === word[0]) return true;
    if (/^[0-9]+$/.test(word) && status.startsWith(word)) return true;
  }
  if (e.error) return word === "error" || word === "failed" || errorText(e).toLowerCase().includes(word);
  return false;
}

export class MockHistory implements HistoryApi {
  /** Oldest first. */
  private entries: HistoryEntry[] = [];
  private next = 0;
  /** When the next entry is recorded; tests may set it. Default: now. */
  clock: () => number = () => Date.now();

  /** Called by `MockBackend.send` after every send. */
  record(collection: string, spec: RequestSpec, result: StoredResponse): HistoryEntry {
    const r = result.report;
    const entry: HistoryEntry = {
      id: ++this.next,
      at: this.clock(),
      collection,
      request_path: r.request_path,
      name: spec.name,
      environment: r.environment,
      method: spec.method,
      url: r.request?.url ?? null,
      status: r.response?.status ?? null,
      error: r.error,
      duration_ms: r.response?.total ?? null,
      size: r.response?.size ?? null,
    };
    this.entries.push(entry);
    if (this.entries.length > HISTORY_LIMIT) this.entries.splice(0, this.entries.length - HISTORY_LIMIT);
    return entry;
  }

  async list(query: string, limit: number): Promise<HistoryEntry[]> {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return this.entries
      .filter((e) => e.collection === MOCK_COLLECTION && words.every((w) => matchesWord(e, w)))
      .reverse()
      .slice(0, limit);
  }

  async clear(): Promise<void> {
    this.entries = this.entries.filter((e) => e.collection !== MOCK_COLLECTION);
  }
}

const RANK: Record<SearchField, number> = {
  name: 0,
  url: 1,
  param: 2,
  header: 2,
  body: 2,
  auth: 2,
  variable: 2,
};

function fields(spec: RequestSpec): [SearchField, string][] {
  const out: [SearchField, string][] = [
    ["name", spec.name],
    ["url", spec.url],
  ];
  for (const p of spec.params) out.push(["param", `${p.name}=${p.value}`]);
  for (const h of spec.headers) out.push(["header", `${h.name}: ${h.value}`]);
  const b = spec.body;
  if (b.type === "raw") out.push(["body", b.data]);
  if (b.type === "form-url-encoded") for (const f of b.fields) out.push(["body", `${f.name}=${f.value}`]);
  if (b.type === "multipart")
    for (const p of b.parts) out.push(["body", p.kind === "text" ? `${p.name}=${p.values.join(", ")}` : p.name]);
  const a = spec.auth;
  if (a.type === "basic") out.push(["auth", `username: ${a.username}`]);
  if (a.type === "bearer") out.push(["auth", `token: ${a.token}`]);
  if (a.type === "api-key") out.push(["auth", `${a.key}: ${a.value}`]);
  for (const v of spec.vars) out.push(["variable", v.value != null && !v.secret ? `${v.name}=${v.value}` : v.name]);
  return out;
}

/** Snippet of the line with the first match of `needle` (lowercase) in `text`. */
function snippet(text: string, needle: string): { snippet: string; ranges: [number, number][] } | null {
  const lower = text.toLowerCase();
  const first = lower.indexOf(needle);
  if (first < 0) return null;
  const lineStart = Math.max(lower.lastIndexOf("\n", first), lower.lastIndexOf("\r", first)) + 1;
  const ends = [lower.indexOf("\n", first), lower.indexOf("\r", first)].filter((i) => i >= 0);
  const lineEnd = ends.length ? Math.min(...ends) : text.length;
  const raw = text.slice(lineStart, lineEnd);
  const lead = raw.length - raw.trimStart().length;
  const line = raw.trim();
  const lowerLine = line.toLowerCase();
  const matches: [number, number][] = [];
  for (let i = lowerLine.indexOf(needle); i >= 0; i = lowerLine.indexOf(needle, i + needle.length)) {
    matches.push([i, i + needle.length]);
  }
  let start = 0;
  let end = line.length;
  if (line.length > SNIPPET_CHARS) {
    start = Math.min(Math.max(0, first - lineStart - lead - CONTEXT_BEFORE), line.length - SNIPPET_CHARS);
    end = Math.max(start + SNIPPET_CHARS, matches[0]?.[1] ?? 0);
  }
  const prefix = start > 0 ? "…" : "";
  const shown = prefix + line.slice(start, end).replace(/[\u0000-\u001f]/g, " ") + (end < line.length ? "…" : "");
  const ranges = matches
    .filter(([s, e]) => s >= start && e <= end)
    .map(([s, e]) => [s - start + prefix.length, e - start + prefix.length] as [number, number]);
  return { snippet: shown, ranges };
}

/** Full-text search over the mock's requests (in their map order). */
export function mockSearch(requests: ReadonlyMap<string, RequestSpec>, query: string, limit: number): SearchHit[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const hits: (SearchHit & { rank: number; order: number })[] = [];
  let order = 0;
  for (const [path, spec] of requests) {
    for (const [field, text] of fields(spec)) {
      const s = snippet(text, needle);
      if (s) hits.push({ path, name: spec.name, method: spec.method, field, ...s, rank: RANK[field], order: order++ });
    }
  }
  return hits
    .sort((a, b) => a.rank - b.rank || a.order - b.order)
    .slice(0, Math.min(limit, SEARCH_LIMIT))
    .map(({ rank: _rank, order: _order, ...hit }) => hit);
}
