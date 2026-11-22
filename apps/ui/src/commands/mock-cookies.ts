// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The cookie jar in the mock backend (wave 5, strand D owns this file). A small model of the
// core's jar (crates/rt-core/src/cookies.rs) for UI tests: one jar per environment (`""`:
// none), host-only cookies with `Path`, `Max-Age`, `Secure`, `HttpOnly` and `SameSite`. The
// mock's send uses `header` and `store`; a request to `…/cookies/set?name=value&…` sets one
// cookie per query parameter, like rt-testserver.

import type { CookieApi } from "./backend";
import type { CookieInfo } from "./types";

export interface MockCookieJar extends CookieApi {
  /** The `Cookie` header for a send to `url` in `environment`, or `null`. */
  header(environment: string, url: string): string | null;
  /** Stores the `Set-Cookie` values of a response to `url`. */
  store(environment: string, url: string, setCookie: string[]): void;
}

/** `name=***` for each cookie: what the core's report shows of a `Cookie` header. */
export function maskCookieHeader(value: string): string {
  return value
    .split(";")
    .map((part) => part.trim())
    .filter((part) => part !== "")
    .map((part) => `${part.split("=")[0]}=***`)
    .join("; ");
}

/** The value hidden, the attributes kept: what the core's report shows of a `Set-Cookie`. */
export function maskSetCookie(value: string): string {
  const [pair, ...attributes] = value.split(";");
  const name = pair.split("=")[0].trim();
  return [`${name}=***`, ...attributes.map((a) => a.trim())].join("; ");
}

/** The `Set-Cookie` headers the mock's `/cookies/set` answers with, or none. */
export function setCookiesFor(url: string): string[] {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return [];
  }
  if (!parsed.pathname.endsWith("/cookies/set")) return [];
  return [...parsed.searchParams].map(([name, value]) => `${name}=${value}; Path=/`);
}

function parse(header: string, url: URL, now: number): CookieInfo | "expire" | null {
  const [pair, ...attributes] = header.split(";");
  const eq = pair.indexOf("=");
  if (eq <= 0) return null;
  const cookie: CookieInfo = {
    name: pair.slice(0, eq).trim(),
    value: pair.slice(eq + 1).trim(),
    domain: url.hostname.toLowerCase(),
    path: "/",
    expires: null,
    secure: false,
    http_only: false,
    same_site: null,
  };
  for (const attribute of attributes) {
    const [rawKey, ...rest] = attribute.split("=");
    const key = rawKey.trim().toLowerCase();
    const value = rest.join("=").trim();
    if (key === "path" && value.startsWith("/")) cookie.path = value;
    else if (key === "max-age" && /^-?\d+$/.test(value)) cookie.expires = now + Number(value) * 1000;
    else if (key === "secure") cookie.secure = true;
    else if (key === "httponly") cookie.http_only = true;
    else if (key === "samesite") cookie.same_site = value;
  }
  return cookie.expires !== null && cookie.expires <= now ? "expire" : cookie;
}

function pathMatches(cookiePath: string, requestPath: string): boolean {
  return (
    requestPath === cookiePath ||
    (requestPath.startsWith(cookiePath) && (cookiePath.endsWith("/") || requestPath[cookiePath.length] === "/"))
  );
}

export function mockCookies(now: () => number = Date.now): MockCookieJar {
  const jars = new Map<string, CookieInfo[]>();
  const live = (environment: string) =>
    (jars.get(environment) ?? []).filter((c) => c.expires === null || c.expires > now());
  const same = (a: CookieInfo, b: CookieInfo) => a.name === b.name && a.domain === b.domain && a.path === b.path;
  return {
    list: async (environment) =>
      live(environment).sort(
        (a, b) => a.domain.localeCompare(b.domain) || a.path.localeCompare(b.path) || a.name.localeCompare(b.name),
      ),
    delete: async (environment, domain, path, name) => {
      jars.set(
        environment,
        (jars.get(environment) ?? []).filter((c) => !(c.domain === domain && c.path === path && c.name === name)),
      );
    },
    clear: async (environment) => {
      jars.delete(environment);
    },
    header(environment, url) {
      let parsed: URL;
      try {
        parsed = new URL(url);
      } catch {
        return null;
      }
      const host = parsed.hostname.toLowerCase();
      const sent = live(environment).filter(
        (c) =>
          c.domain === host &&
          pathMatches(c.path, parsed.pathname || "/") &&
          (!c.secure || parsed.protocol === "https:"),
      );
      sent.sort((a, b) => b.path.length - a.path.length);
      return sent.length === 0 ? null : sent.map((c) => `${c.name}=${c.value}`).join("; ");
    },
    store(environment, url, setCookie) {
      let parsed: URL;
      try {
        parsed = new URL(url);
      } catch {
        return;
      }
      let jar = live(environment);
      for (const header of setCookie) {
        const cookie = parse(header, parsed, now());
        if (cookie === null) continue;
        if (cookie === "expire") {
          const name = header.split("=")[0].trim();
          jar = jar.filter((c) => !(c.name === name && c.domain === parsed.hostname.toLowerCase()));
          continue;
        }
        jar = [...jar.filter((c) => !same(c, cookie)), cookie];
      }
      if (jar.length === 0) jars.delete(environment);
      else jars.set(environment, jar);
    },
  };
}
