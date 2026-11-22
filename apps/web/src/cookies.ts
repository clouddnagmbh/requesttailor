// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CookieApi` of the web app (wave 5, strand D). The browser's `fetch` keeps its own cookies,
// forbids the `Cookie` header and hides `Set-Cookie`, so the app's jar applies only to sends
// through the companion: `forSend` hands the jar to the core (crates/rt-wasm/src/cookies_js.rs)
// only while the companion is paired.
//
// - One jar per collection, scopes per environment (`""`: none), in the core's `CookieJars`
//   JSON (`{ version: 1, scopes: { Local: [cookie, ...] } }`), the format of desktop and CLI.
// - The jars live in memory for the page session only: cookies are session state and often
//   credentials, and the browser has no keychain. Reloading the page empties them.

import type { CookieApi, CookieInfo } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

/** One cookie as the core stores it (`rt_core::cookies::Cookie`). */
export interface StoredCookie extends CookieInfo {
  host_only: boolean;
  created: number;
}

/** `rt_core::cookies::CookieJars`. */
export interface CookieJars {
  version: number;
  scopes: Record<string, StoredCookie[]>;
}

const JARS_VERSION = 1;

export function emptyJars(): CookieJars {
  return { version: JARS_VERSION, scopes: {} };
}

/** Parses jars the core wrote; anything else is an empty jar (as in the core). */
export function parseJars(json: string): CookieJars {
  try {
    const jars = JSON.parse(json) as CookieJars;
    if (jars && jars.version === JARS_VERSION && typeof jars.scopes === "object" && jars.scopes) {
      return jars;
    }
  } catch {
    // fall through
  }
  return emptyJars();
}

/** The live cookies of `environment` at `now` (Unix ms), sorted like `rt_app::cookies::list`. */
export function listCookies(jars: CookieJars, environment: string, now: number): CookieInfo[] {
  const key = (c: CookieInfo) => [c.domain, c.path, c.name];
  return (jars.scopes[environment] ?? [])
    .filter((c) => c.expires === null || c.expires > now)
    .map(
      ({ name, value, domain, path, expires, secure, http_only, same_site }): CookieInfo => ({
        name,
        value,
        domain,
        path,
        expires,
        secure,
        http_only,
        same_site,
      }),
    )
    .sort((a, b) => {
      const [ka, kb] = [key(a), key(b)];
      for (let i = 0; i < ka.length; i++) {
        if (ka[i] !== kb[i]) return ka[i] < kb[i] ? -1 : 1;
      }
      return 0;
    });
}

/** Removes one cookie; drops a scope that becomes empty. */
export function removeCookie(
  jars: CookieJars,
  environment: string,
  domain: string,
  path: string,
  name: string,
): CookieJars {
  const scopes = { ...jars.scopes };
  const rest = (scopes[environment] ?? []).filter(
    (c) => !(c.domain === domain && c.path === path && c.name === name),
  );
  if (rest.length > 0) scopes[environment] = rest;
  else delete scopes[environment];
  return { ...jars, scopes };
}

export function clearScope(jars: CookieJars, environment: string): CookieJars {
  const scopes = { ...jars.scopes };
  delete scopes[environment];
  return { ...jars, scopes };
}

/** The `cookies` field of the services object `Workspace.send` reads. */
export interface CoreCookieJar {
  jars: string;
  save(jars: string): void;
}

/** `CookieApi` plus the jar of a send (the `cookies` field `services_js` reads). */
export interface WebCookieApi extends CookieApi {
  forSend(collectionId: string, environment: string | null): Promise<CoreCookieJar | undefined>;
}

export function webCookies(
  host: Pick<WebHost, "session" | "companionCredentials">,
  now: () => number = Date.now,
): WebCookieApi {
  const byCollection = new Map<string, CookieJars>();
  const current = (): string | null => {
    try {
      return host.session().id;
    } catch {
      return null;
    }
  };
  const update = (f: (jars: CookieJars) => CookieJars) => {
    const id = current();
    if (id === null) return;
    const next = f(byCollection.get(id) ?? emptyJars());
    if (Object.keys(next.scopes).length === 0) byCollection.delete(id);
    else byCollection.set(id, next);
  };
  return {
    list: async (environment) => {
      const id = current();
      const jars = id === null ? undefined : byCollection.get(id);
      return jars ? listCookies(jars, environment, now()) : [];
    },
    delete: async (environment, domain, path, name) =>
      update((jars) => removeCookie(jars, environment, domain, path, name)),
    clear: async (environment) => update((jars) => clearScope(jars, environment)),
    forSend: async (collectionId) => {
      // Without the companion the browser sends: it would drop the header and hide the answer.
      if ((await host.companionCredentials()) === null) return undefined;
      return {
        jars: JSON.stringify(byCollection.get(collectionId) ?? emptyJars()),
        save: (json: string) => {
          const jars = parseJars(json);
          if (Object.keys(jars.scopes).length === 0) byCollection.delete(collectionId);
          else byCollection.set(collectionId, jars);
        },
      };
    },
  };
}
