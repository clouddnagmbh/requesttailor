// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the history view: relative times and status labels.

import type { HistoryEntry } from "../commands/types";
import { statusClass, type StatusClass } from "../response/format";

/** "just now", "2 min ago", "3 h ago", "yesterday", "4 days ago", then the date. */
export function timeAgo(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 45) return "just now";
  const min = Math.max(1, Math.round(s / 60));
  if (min < 60) return `${min} min ago`;
  const h = Math.round(min / 60);
  if (h < 24) return `${h} h ago`;
  const days = Math.round(h / 24);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  return new Date(at).toISOString().slice(0, 10);
}

/** Colour class of an entry: the status class, or `error` without a response. */
export function entryClass(entry: HistoryEntry): StatusClass | "error" {
  return entry.status == null ? "error" : statusClass(entry.status);
}

/** What the status column shows: the code, or "Error" without a response. */
export function statusLabel(entry: HistoryEntry): string {
  return entry.status == null ? "Error" : String(entry.status);
}
