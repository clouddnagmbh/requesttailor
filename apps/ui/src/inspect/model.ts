// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: what the JWT view, the send warning and the checks chip show, as plain
// functions. A token's value is shown by the view that was asked for it and kept nowhere; times
// and spans are only presentation of what the core measured (`JwtInfo.warnings`, claims).

import type { FoundToken, JwtClaim, JwtWarning, Observation, TokenLocation } from "../commands/types";

/** "1 check", "3 checks". */
export function chipLabel(n: number): string {
  return n === 1 ? "1 check" : `${n} checks`;
}

/** The chip is calm: `warn` only if an observation says so. */
export function chipTone(observations: Observation[]): "warn" | "info" {
  return observations.some((o) => o.severity === "warn") ? "warn" : "info";
}

/** Warnings first, then in the order of the rules. */
export function sortObservations(observations: Observation[]): Observation[] {
  const rank = (o: Observation) => (o.severity === "warn" ? 0 : 1);
  return observations.map((o, i) => ({ o, i })).sort((a, b) => rank(a.o) - rank(b.o) || a.i - b.i).map((x) => x.o);
}

/** `12 s`, `12 min`, `3 h`, `2 d` (rounded down), like the core's `jwt::span`. */
export function span(seconds: number): string {
  const s = Math.floor(Math.abs(seconds));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h`;
  return `${Math.floor(s / 86_400)} d`;
}

/** `in 12 min` or `12 min ago` for a time `delta` seconds from now. */
export function relative(delta: number): string {
  if (Math.abs(delta) < 1) return "now";
  return delta > 0 ? `in ${span(delta)}` : `${span(delta)} ago`;
}

/** A Unix time as an ISO date (UTC, to the second) and its relative time. */
export function timeText(seconds: number, now: number): { date: string; relative: string } {
  const d = new Date(seconds * 1000);
  const date = Number.isNaN(d.getTime()) ? String(seconds) : d.toISOString().replace(".000Z", "Z");
  return { date, relative: relative(seconds - now) };
}

/** The countdown of a token: to the second below an hour, then coarser. */
export function countdown(expiresAt: number, now: number): string {
  const left = expiresAt - now;
  if (left <= 0) return `expired ${span(left)} ago`;
  if (left < 60) return `expires in ${left} s`;
  if (left < 3600) return `expires in ${Math.floor(left / 60)} min ${left % 60} s`;
  return `expires in ${span(left)}`;
}

/** Whether the countdown is about to change at every tick (under an hour either way). */
export function ticksFast(expiresAt: number, now: number): boolean {
  return Math.abs(expiresAt - now) < 3600;
}

/** A claim's value as one line of text. */
export function claimText(claim: JwtClaim): string {
  const v = claim.value;
  if (typeof v === "string") return v;
  if (Array.isArray(v) && v.every((x) => typeof x === "string")) return v.join(", ");
  return JSON.stringify(v);
}

/** The words of a scope claim, or `null` if it is not text. */
export function scopeWords(claim: JwtClaim): string[] | null {
  return typeof claim.value === "string" ? claim.value.split(/\s+/).filter(Boolean) : null;
}

/** Where a token was found, as words. */
export function locationText(location: TokenLocation): string {
  switch (location.kind) {
    case "authorization":
      return "Authorization header";
    case "auth":
      return "Auth block";
    case "query":
      return "access_token parameter";
    case "body":
      return `response body ${location.path}`;
    case "cookie":
      return `cookie ${location.name}`;
  }
}

/** The one warning worth stopping for before a send: expired, not yet valid, or expiring in
 *  under 30 seconds. `alg: none` is not a reason to hold back a request the user wrote. */
export function sendWarning(found: FoundToken[]): { warning: JwtWarning; where: TokenLocation } | null {
  for (const f of found) {
    const warning = f.token.warnings.find((w) => w.kind === "expired" || w.kind === "not-yet-valid" || w.kind === "expires-soon");
    if (warning) return { warning, where: f.location };
  }
  return null;
}

/** "Token expired 12 min ago." from the core's "token expired 12 min ago (exp)". */
export function sentence(text: string): string {
  const t = text.replace(/\s*\((exp|nbf)\)\s*$/, "").trim();
  return (t.charAt(0).toUpperCase() + t.slice(1)).replace(/[.]?$/, ".");
}
