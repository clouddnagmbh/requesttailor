// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Display helpers for the cookie dialog (wave 5, strand D).

import type { CookieInfo } from "../commands/types";

/** `expires` (Unix milliseconds) as local date and time; `Session` for session cookies. */
export function formatExpires(expires: number | null, now: number = Date.now()): string {
  if (expires === null) return "Session";
  const d = new Date(expires);
  const pad = (n: number) => String(n).padStart(2, "0");
  const text = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return expires <= now ? `${text} (expired)` : text;
}

/** Secure, HttpOnly and SameSite as short labels. */
export function cookieFlags(c: CookieInfo): string[] {
  const flags: string[] = [];
  if (c.secure) flags.push("Secure");
  if (c.http_only) flags.push("HttpOnly");
  if (c.same_site) flags.push(`SameSite=${c.same_site}`);
  return flags;
}

/** Stable order: domain, path, name. */
export function sortCookies(cookies: CookieInfo[]): CookieInfo[] {
  return [...cookies].sort(
    (a, b) => a.domain.localeCompare(b.domain) || a.path.localeCompare(b.path) || a.name.localeCompare(b.name),
  );
}

/** Identity of a cookie in the jar (RFC 6265: name, domain and path). */
export const cookieKey = (c: CookieInfo) => `${c.domain}\u0000${c.path}\u0000${c.name}`;
