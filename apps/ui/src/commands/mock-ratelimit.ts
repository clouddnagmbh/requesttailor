// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand D: rate limits in the mock backend. `?ratelimit=1` mirrors a server that
// announces `RateLimit-*` headers (`47 / 60 left, resets in 12 s`), `?ratelimit=exhausted`
// one that has nothing left until the reset (1.5 s, so runner tests stay quick), `?ratelimit=long`
// the same with 2 minutes (longer than a run waits by default),
// `?ratelimit=retry` one that asks to come back later (`Retry-After`, 12 s).

import type { RateLimits } from "./types";

export function mockRateLimit(): RateLimits | undefined {
  const mode = new URLSearchParams(globalThis.location?.search ?? "").get("ratelimit");
  switch (mode) {
    case "1":
      return { limit: 60, remaining: 47, reset_ms: 12_000, windows: [{ limit: 60, seconds: 60 }] };
    case "exhausted":
      return { limit: 60, remaining: 0, reset_ms: 1500 };
    case "long":
      return { limit: 60, remaining: 0, reset_ms: 120_000 };
    case "retry":
      return { retry_after_ms: 12_000 };
    default:
      return undefined;
  }
}

/** How long a throttled mock run holds the next request back after a response that said
 *  `limits` (the core's `RateTracker::wait`, for the one host the mock has). */
export function mockWaitMs(limits: RateLimits | undefined): number {
  if (!limits) return 0;
  const reset = limits.remaining === 0 ? (limits.reset_ms ?? 0) : 0;
  return Math.max(limits.retry_after_ms ?? 0, reset);
}
