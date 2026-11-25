// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand D: how rate limits read in the interface, without Svelte (unit-tested).
// The numbers come from the core (`rt_core::ratelimit`, `SendReport.rate_limit`); nothing is
// parsed or computed from headers here.

import type { RateLimits } from "../commands/types";

/** Whole seconds, rounded up, like the core's reasons: `12 s`. */
export function secondsText(ms: number): string {
  return `${Math.ceil(Math.max(0, ms) / 1000)} s`;
}

export type RateTone = "ok" | "warn" | "bad";

export interface RateChipData {
  /** `47 / 60 left, resets in 12 s`, `Rate limited, retry in 12 s`, `Rate limited`. */
  text: string;
  tone: RateTone;
  /** Longer description for the tooltip. */
  title: string;
}

/** What the chip in the status line says for the limits of a response with `status`. */
export function rateChip(limits: RateLimits, status: number | undefined): RateChipData {
  const { limit, remaining, reset_ms: reset, retry_after_ms: retry, windows } = limits;
  const resets = reset !== undefined ? `resets in ${secondsText(reset)}` : undefined;
  const policy = (windows ?? []).map((w) => `${w.limit} per ${w.seconds} s`).join(", ");
  const title = ["Rate limit announced by the server", policy ? `Windows: ${policy}` : ""].filter(Boolean).join(". ");

  if (retry !== undefined) {
    return { text: `Rate limited, retry in ${secondsText(retry)}`, tone: "bad", title };
  }
  if (status === 429 && remaining === undefined) {
    return { text: resets ? `Rate limited, ${resets}` : "Rate limited", tone: "bad", title };
  }
  const left =
    remaining !== undefined
      ? limit !== undefined
        ? `${remaining} / ${limit} left`
        : `${remaining} left`
      : limit !== undefined
        ? `limit ${limit}`
        : undefined;
  const text = [left, resets].filter(Boolean).join(", ") || "Rate limit";
  let tone: RateTone = "ok";
  if (remaining === 0) tone = "bad";
  else if (remaining !== undefined && limit !== undefined && limit > 0 && remaining / limit <= 0.1) tone = "warn";
  return { text: text.charAt(0).toUpperCase() + text.slice(1), tone, title };
}

/** The runner's waiting line: `Waiting 12 s (rate limit)`, counted down from `until` (unix ms). */
export function waitingText(until: number, now: number): string {
  return `Waiting ${secondsText(until - now)} (rate limit)`;
}
