// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the snapshot view needs besides the backend: turning a difference's path into an ignore
// path, and the words for each state.

import type { SnapshotCheck, SnapshotState } from "../commands/types";

/** The ignore path for a difference at `path` (`status`, `headers.x`, `body.items[0].id`);
 *  `everyItem` turns every array index into `[*]`. */
export function ignorePath(path: string, everyItem = false): string {
  const p = everyItem ? path.replace(/\[\d+\]/g, "[*]") : path;
  if (p === "body") return "$";
  if (p.startsWith("body.") || p.startsWith("body[")) return `$${p.slice(4)}`;
  return `res.${p}`;
}

/** Whether `ignorePath` has a second form with wildcards. */
export function hasIndex(path: string): boolean {
  return /\[\d+\]/.test(path);
}

/** Adds `path` to `ignore` unless it is there. */
export function withIgnore(ignore: string[], path: string): string[] {
  return ignore.includes(path) ? ignore : [...ignore, path];
}

export function without(ignore: string[], path: string): string[] {
  return ignore.filter((p) => p !== path);
}

export function stateLabel(check: Pick<SnapshotCheck, "state" | "differences">): string {
  const n = check.differences.length;
  const labels: Record<SnapshotState, string> = {
    matches: "Matches snapshot",
    differs: `${n} ${n === 1 ? "difference" : "differences"} from snapshot`,
    missing: "Snapshot file missing",
    none: "No snapshot",
  };
  return labels[check.state];
}

/** The text shown for a value of a difference. */
export function show(v: unknown): string {
  if (v === null || v === undefined) return "nothing";
  const t = typeof v === "string" ? JSON.stringify(v) : (JSON.stringify(v) ?? String(v));
  return t.length > 120 ? `${t.slice(0, 119)}…` : t;
}
