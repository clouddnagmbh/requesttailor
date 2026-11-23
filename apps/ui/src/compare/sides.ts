// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Helpers of the side-by-side answers (wave 14, strand H): headers of both sides as rows, the
// time bars, sizes and status colours.

/** Headers that change on every response; shown, but never counted as a change. */
export const VOLATILE_HEADERS = new Set([
  "date",
  "age",
  "server-timing",
  "x-request-id",
  "x-correlation-id",
  "x-trace-id",
  "traceparent",
  "content-length",
  "keep-alive",
  "connection",
]);

export interface HeaderRow {
  key: string;
  name: string;
  old: string | null;
  new: string | null;
  kind: "same" | "changed" | "added" | "removed";
  volatile: boolean;
}

/** Both sides' headers by name (any case; repeated headers joined with `, `), sorted by name.
 *  A volatile header that differs is `same` with `volatile` set. `null`: no response there. */
export function headerRows(old: [string, string][] | null, now: [string, string][] | null): HeaderRow[] {
  const fold = (h: [string, string][] | null) => {
    const m = new Map<string, [string, string]>();
    for (const [name, value] of h ?? []) {
      const key = name.toLowerCase();
      const have = m.get(key);
      m.set(key, have ? [have[0], `${have[1]}, ${value}`] : [name, value]);
    }
    return m;
  };
  const [a, b] = [fold(old), fold(now)];
  const keys = [...new Set([...a.keys(), ...b.keys()])].sort();
  return keys.map((key) => {
    const [o, n] = [a.get(key), b.get(key)];
    const volatile = VOLATILE_HEADERS.has(key);
    const kind = !o ? "added" : !n ? "removed" : o[1] === n[1] || volatile ? "same" : "changed";
    // A header on one side only counts when both sides answered.
    const counted = old !== null && now !== null ? kind : "same";
    return { key, name: (n ?? o)![0], old: o?.[1] ?? null, new: n?.[1] ?? null, kind: volatile ? "same" : counted, volatile };
  });
}

export interface TimeBar {
  ms: number;
  /** Width relative to the slower side, 2…100. */
  percent: number;
}

export interface TimeBars {
  old: TimeBar | null;
  new: TimeBar | null;
  /** The newer side is clearly slower (20 % and 50 ms) or faster. */
  slower: boolean;
  faster: boolean;
  /** `+120 ms (2.5× slower)`; `null` when one side is missing. */
  delta: string | null;
}

export function timeBars(old: number | null, now: number | null): TimeBars {
  const max = Math.max(old ?? 0, now ?? 0, 1);
  const bar = (ms: number | null): TimeBar | null => (ms === null ? null : { ms, percent: Math.max(2, Math.round((ms / max) * 100)) });
  if (old === null || now === null) return { old: bar(old), new: bar(now), slower: false, faster: false, delta: null };
  const diff = now - old;
  const clear = Math.abs(diff) >= 50 && Math.abs(diff) >= 0.2 * Math.min(old, now);
  const ratio = Math.min(old, now) > 0 ? Math.max(old, now) / Math.min(old, now) : null;
  const how = ratio && ratio >= 1.5 ? ` (${ratio.toFixed(1)}× ${diff > 0 ? "slower" : "faster"})` : "";
  return {
    old: bar(old),
    new: bar(now),
    slower: clear && diff > 0,
    faster: clear && diff < 0,
    delta: diff === 0 ? "Same time" : `${diff > 0 ? "+" : "−"}${Math.abs(diff)} ms${how}`,
  };
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** `ok` 2xx, `warn` 3xx, `bad` 4xx and 5xx (and anything else). */
export function statusClass(status: number): "ok" | "warn" | "bad" {
  return status >= 200 && status < 300 ? "ok" : status >= 300 && status < 400 ? "warn" : "bad";
}
