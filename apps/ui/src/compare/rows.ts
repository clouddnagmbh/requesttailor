// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The rows of the graphical comparison (wave 14, strand H): one per request, joining what
// changed in its definition (the semantic diff of the two revisions), how its answers differ
// (the runs at both revisions) and the breaking changes of the API contract that touch it.
// Pure functions, so the counts of the header, the filters and the n/p keys are tested here.

import type { CollectionDiff, ModelChange, RequestComparison, RunComparison, SpecChange } from "../commands/types";

/** `new`: only at the newer revision; `removed`: only at the older one. */
export type RowState = "new" | "removed" | "changed" | "same";

export interface CompareRow {
  /** Path at the newer revision (the older one for removed requests). */
  path: string;
  /** Path at the older revision when the request was moved or renamed. */
  oldPath?: string;
  name: string;
  state: RowState;
  /** The request's definition change; `null`: the file is the same at both revisions. */
  definition: ModelChange | null;
  /** The answers of both runs; `null` when the requests were not sent. */
  response: RequestComparison | null;
  /** Breaking changes of the contract that name this request. */
  breaking: SpecChange[];
  /** Tests or assertions that passed at the older revision and fail at the newer one. */
  regressed: string[];
  /** Fails at the newer revision in a way it did not before (a regression, a new error, a
   *  test that only runs now and fails). */
  failing: boolean;
}

export interface HeaderCounts {
  total: number;
  changed: number;
  added: number;
  removed: number;
  same: number;
  /** Requests a breaking contract change touches. */
  breaking: number;
  /** Requests with at least one test going from green to red. */
  regressed: number;
  /** Tests going from red to green. */
  fixed: number;
}

/** `all`, `changes` and `failures` are the list's filters; the others are what a tile of the
 *  header narrows to (`changed`, `new`, `removed` by state, `breaking` by contract). */
export type CompareFilter = "all" | "changes" | "failures" | "changed" | "new" | "removed" | "breaking";

function requestChanges(diff: CollectionDiff | null): ModelChange[] {
  return diff ? diff.changes.filter((c) => c.target.kind === "request") : [];
}

const pathOf = (c: ModelChange): string => (c.target.kind === "request" ? c.target.path : "");

/** Outcomes of a moved request's tests: the older half knows the old ones, the newer half the
 *  new ones; only those that differ. */
function movedTests(older: RequestComparison, newer: RequestComparison): RequestComparison["tests"] {
  const names = [...new Set([...older.tests.map((t) => t.name), ...newer.tests.map((t) => t.name)])];
  return names.flatMap((name) => {
    const old = older.tests.find((t) => t.name === name)?.old ?? null;
    const now = newer.tests.find((t) => t.name === name)?.new ?? null;
    return old === now ? [] : [{ name, old, new: now }];
  });
}

/** A comparison of two answers whose sides come from two rows (a moved request runs under its
 *  old path at the older revision and its new path at the newer one). */
function joinMoved(older: RequestComparison | undefined, newer: RequestComparison | undefined): RequestComparison | null {
  if (!older || !newer) return newer ?? older ?? null;
  const oldSide = older.sides?.old ?? null;
  const newSide = newer.sides?.new ?? null;
  const status = (oldSide?.status ?? null) !== (newSide?.status ?? null) ? { old: oldSide?.status ?? null, new: newSide?.status ?? null } : null;
  return {
    ...newer,
    same: false,
    status,
    // The change lists of the two halves compare against nothing; the side-by-side view
    // computes headers and body from the sides.
    headers: [],
    body: [],
    body_text: false,
    tests: movedTests(older, newer),
    error: older.error?.old || newer.error?.new ? { old: older.error?.old ?? null, new: newer.error?.new ?? null } : null,
    sides: { old: oldSide, new: newSide },
  };
}

/**
 * Joins the semantic diff (`diffs.between(from, to)`), the comparison of the runs and the
 * contract changes into one row per request, in the order of the runs (the collection's run
 * order), then the requests only the diff knows. Either input may be missing.
 */
export function buildRows(diff: CollectionDiff | null, run: RunComparison | null, contract: SpecChange[] = []): CompareRow[] {
  const changes = requestChanges(diff);
  const byPath = new Map(changes.map((c) => [pathOf(c), c]));
  const movedFrom = new Map<string, ModelChange>();
  for (const c of changes) if (c.change.kind === "moved") movedFrom.set(c.change.from, c);
  const responses = new Map((run?.requests ?? []).map((r) => [r.path, r]));
  const breaking = contract.filter((c) => c.breaking);
  const breakingOf = (...paths: (string | undefined)[]) => breaking.filter((c) => paths.some((p) => p !== undefined && c.requests.includes(p)));

  const rows: CompareRow[] = [];
  const seen = new Set<string>();
  const add = (path: string, definition: ModelChange | null, response: RequestComparison | null, name: string) => {
    const oldPath = definition?.change.kind === "moved" ? definition.change.from : undefined;
    seen.add(path);
    if (oldPath) seen.add(oldPath);
    const regressed = (response?.tests ?? []).filter((t) => t.old === true && t.new === false).map((t) => t.name);
    const failing =
      regressed.length > 0 ||
      (response?.tests ?? []).some((t) => t.old === null && t.new === false) ||
      (!!response?.error?.new && !response.error.old);
    let state: RowState;
    const kind = definition?.change.kind;
    if (kind === "added") state = "new";
    else if (kind === "removed") state = "removed";
    else if (kind === "modified" || kind === "moved") state = "changed";
    else if (response && !response.same) {
      const sides = response.sides;
      // A request the diff does not know is missing on one side only when it was not run there.
      if (sides && !sides.old && sides.new && !response.error) state = diff ? "changed" : "new";
      else if (sides && sides.old && !sides.new && !response.error) state = diff ? "changed" : "removed";
      else state = "changed";
    } else state = "same";
    rows.push({ path, ...(oldPath ? { oldPath } : {}), name, state, definition, response, breaking: breakingOf(path, oldPath), regressed, failing });
  };

  for (const r of run?.requests ?? []) {
    if (seen.has(r.path)) continue;
    const moved = movedFrom.get(r.path);
    if (moved) {
      // The old path of a moved request: its row is the new path's.
      const to = pathOf(moved);
      add(to, moved, joinMoved(r, responses.get(to)), moved.name || r.name);
      continue;
    }
    const definition = byPath.get(r.path) ?? null;
    if (definition?.change.kind === "moved") {
      add(r.path, definition, joinMoved(responses.get(definition.change.from), r), definition.name || r.name);
      continue;
    }
    add(r.path, definition, r, r.name || definition?.name || r.path);
  }
  for (const c of changes) {
    const path = pathOf(c);
    if (!seen.has(path)) add(path, c, null, c.name || path);
  }
  return rows;
}

export function headerCounts(rows: CompareRow[]): HeaderCounts {
  const counts: HeaderCounts = { total: rows.length, changed: 0, added: 0, removed: 0, same: 0, breaking: 0, regressed: 0, fixed: 0 };
  for (const r of rows) {
    if (r.state === "new") counts.added++;
    else if (r.state === "removed") counts.removed++;
    else if (r.state === "changed") counts.changed++;
    else counts.same++;
    if (r.breaking.length > 0) counts.breaking++;
    if (r.regressed.length > 0) counts.regressed++;
    counts.fixed += (r.response?.tests ?? []).filter((t) => t.old === false && t.new === true).length;
  }
  return counts;
}

/** A row the filter "changes" keeps: anything but `same` (a breaking change counts too). */
export const isChange = (r: CompareRow): boolean => r.state !== "same" || r.breaking.length > 0;

/** The rows a filter and a search text keep (the text matches name or path, any case). */
export function filterRows(rows: CompareRow[], filter: CompareFilter, query = ""): CompareRow[] {
  const q = query.trim().toLowerCase();
  return rows.filter((r) => {
    if (filter === "changes" && !isChange(r)) return false;
    if (filter === "failures" && !r.failing) return false;
    if ((filter === "changed" || filter === "new" || filter === "removed") && r.state !== filter) return false;
    if (filter === "breaking" && r.breaking.length === 0) return false;
    if (!q) return true;
    return r.name.toLowerCase().includes(q) || r.path.toLowerCase().includes(q) || (r.oldPath?.toLowerCase().includes(q) ?? false);
  });
}

/**
 * The index of the next (`step` 1) or previous (`step` -1) changed row from `index`, wrapping
 * around; `-1` when no row changed. From `index` -1 the first (or last) change is found.
 */
export function nextChange(rows: CompareRow[], index: number, step: 1 | -1): number {
  const n = rows.length;
  for (let k = 1; k <= n; k++) {
    const i = index < 0 ? (step === 1 ? k - 1 : n - k) : (((index + step * k) % n) + n) % n;
    if (isChange(rows[i]!)) return i;
  }
  return -1;
}

/** What differs in a row, in a few plain words, for its line in the list. */
export function rowSummary(r: CompareRow): string {
  if (r.state === "new") return "new at the newer revision";
  if (r.state === "removed") return "not at the newer revision";
  const parts: string[] = [];
  if (r.oldPath) parts.push(`moved from ${r.oldPath}`);
  const fields = r.definition?.fields.length ?? 0;
  if (fields > 0) parts.push(`${fields} ${fields === 1 ? "field" : "fields"} edited`);
  const resp = r.response;
  if (resp && !resp.same) {
    if (resp.status) parts.push(`status ${resp.status.old ?? "none"} → ${resp.status.new ?? "none"}`);
    if (resp.error) parts.push(resp.error.new ? "fails to send" : "sends again");
    const answer = resp.headers.length + resp.body.length + (resp.body_text ? 1 : 0);
    if (answer > 0 && !resp.status) parts.push("answer differs");
  }
  if (r.regressed.length > 0) parts.push(`${r.regressed.length} ${r.regressed.length === 1 ? "test" : "tests"} now failing`);
  if (r.breaking.length > 0) parts.push("breaking API change");
  return parts.length > 0 ? parts.join(" · ") : "same at both revisions";
}
