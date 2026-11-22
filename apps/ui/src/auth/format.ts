// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Display helpers for OAuth 2.0 tokens and prompts (wave 5, strand C). Token values never
// reach the UI; only `TokenInfo` (times, refreshable) does.

import type { TokenInfo } from "../commands/types";

/** A rough duration: "42 s", "5 min", "2 h 5 min", "3 d". */
export function duration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s`;
  const min = Math.floor(s / 60);
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  if (h < 24) return min % 60 ? `${h} h ${min % 60} min` : `${h} h`;
  return `${Math.floor(h / 24)} d`;
}

export interface TokenState {
  kind: "valid" | "expired";
  text: string;
}

export function tokenState(token: TokenInfo, now: number): TokenState {
  if (token.expires_at == null) return { kind: "valid", text: "no expiry given" };
  if (token.expires_at > now) return { kind: "valid", text: `valid for ${duration(token.expires_at - now)}` };
  return { kind: "expired", text: token.refreshable ? "expired, refreshed on the next send" : "expired" };
}

/** Local date and time of a Unix-millisecond timestamp. */
export function when(ms: number): string {
  return new Date(ms).toLocaleString();
}

/** `m:ss` left of `expiresIn` seconds counted from `since`. */
export function countdown(expiresIn: number, since: number, now: number): string {
  const left = Math.max(0, Math.ceil(expiresIn - (now - since) / 1000));
  return `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
}
