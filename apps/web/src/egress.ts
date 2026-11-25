// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EgressApi` of the web app (wave 19, strand B): guided network access for extensions over
// rt-wasm (`crates/rt-wasm/src/egress_js.rs`, the same `rt_app::egress` as the desktop app).
// The list of allowed hosts is kept per browser profile in localStorage (a profile that
// refuses storage keeps it in memory for the page). A call goes out with `fetch`; when the
// browser refuses (CORS), it is repeated over the companion (`rt agent`) if one is paired. Every
// call goes through the distribution's send guard (`sendGuard`), with `Purpose::Egress`. Secret
// values stay in the core: only the `{{secret:<name>}}` references of the headers are looked up.

import type { EgressApi, EgressHost, EgressRequest, EgressResponse } from "@requesttailor/ui";
import { BackendError } from "@requesttailor/ui";

import type { SendGuardFn } from "./send-services";
import type { CompanionCredentials } from "./wasm";
import { toBackendError } from "./wasm";

export const HOSTS_KEY = "requesttailor.egress-hosts";

/** The egress part of rt-wasm's exports. */
export interface CoreEgress {
  egressHostsParse(text: string): EgressHost[];
  egressHostsAllow(
    hosts: EgressHost[],
    host: string,
    port: number | null,
    extension: string,
    purpose: string,
    nowMs: number,
  ): { hosts: EgressHost[]; entry: EgressHost };
  egressHostsRemove(hosts: EgressHost[], host: string, port: number | null): EgressHost[];
  egressSecretNames(request: EgressRequest): string[];
  egressRequest(
    request: EgressRequest,
    hosts: EgressHost[],
    secrets: [string, string][],
    collection: string,
    companion: CompanionCredentials | null,
    services: { guard: SendGuardFn } | null,
  ): Promise<EgressResponse>;
}

/** The part of `Storage` this module uses. */
export type HostStorage = Pick<Storage, "getItem" | "setItem">;

export interface EgressDeps {
  core(): Promise<unknown>;
  /** The open collection's name and the values of the secret variables `names` (of
   *  `request.environment`, else the first environment with a value); `names` is never empty. */
  secrets(request: EgressRequest, names: string[]): Promise<{ collection: string; values: [string, string][] }>;
  /** The companion's credentials: while the user has it switched on, and when it is merely
   *  paired. `null` when there is none. */
  companion: { on(): boolean; paired(): Promise<CompanionCredentials | null>; active(): Promise<CompanionCredentials | null> };
  sendGuard(): SendGuardFn | undefined;
  storage?: HostStorage | null;
  now?: () => number;
}

function browserStorage(): HostStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** What `fetch` reports when the browser refuses a call (CORS, mixed content, network). */
export const BLOCKED_PREFIX = "blocked by the browser";

export function webEgress(deps: EgressDeps): EgressApi {
  const storage = deps.storage === undefined ? browserStorage() : deps.storage;
  const now = deps.now ?? (() => Date.now());
  // What this page knows when storage is missing or refuses.
  let memory: string | null = null;

  const core = async () => (await deps.core()) as CoreEgress;

  async function read(): Promise<EgressHost[]> {
    const c = await core();
    let text: string | null = memory;
    try {
      text = storage?.getItem(HOSTS_KEY) ?? memory;
    } catch {
      // The page's own copy.
    }
    // A broken value allows nothing (and `write` below refuses to cover it up unseen).
    return text ? c.egressHostsParse(text) : [];
  }

  function write(hosts: EgressHost[]) {
    const text = JSON.stringify(hosts);
    memory = text;
    try {
      storage?.setItem(HOSTS_KEY, text);
    } catch {
      // Kept for this page.
    }
  }

  async function run(request: EgressRequest): Promise<EgressResponse> {
    const c = await core();
    const hosts = await read();
    const names = c.egressSecretNames(request);
    const found = names.length > 0 ? await deps.secrets(request, names) : { collection: "", values: [] };
    const guard = deps.sendGuard();
    const services = guard ? { guard } : null;
    let credentials = await deps.companion.active();
    // A writing call is never repeated: a refused browser call may still have reached the server.
    // With a paired companion it goes that way at once; otherwise it is tried once, here.
    const writes = request.method !== "GET";
    if (writes && !credentials) credentials = await deps.companion.paired();
    for (;;) {
      try {
        return await c.egressRequest(request, hosts, found.values, found.collection, credentials, services);
      } catch (e) {
        const error = toBackendError(e);
        const refused = error.kind === "transport" && error.message.startsWith(BLOCKED_PREFIX);
        if (refused && writes && !credentials) {
          throw new BackendError(
            "transport",
            `${error.message}. A writing call is not repeated; run \`rt agent\` and turn on the companion to send it that way.`,
          );
        }
        if (!refused || credentials) {
          throw refused ? new BackendError("transport", `${error.message}. The companion could not reach it either.`) : error;
        }
        // The browser refused (CORS): once more over the companion, if one is paired.
        credentials = await deps.companion.paired();
        if (!credentials) {
          throw new BackendError(
            "transport",
            `${error.message}. Run \`rt agent\` and turn on the companion to reach this host.`,
          );
        }
      }
    }
  }

  return {
    hosts: async () => {
      try {
        return await read();
      } catch (e) {
        throw toBackendError(e);
      }
    },
    allow: async (host, port, extension, purpose) => {
      try {
        const c = await core();
        const { hosts, entry } = c.egressHostsAllow(await read(), host, port, extension, purpose, now());
        write(hosts);
        return entry;
      } catch (e) {
        throw toBackendError(e);
      }
    },
    remove: async (host, port) => {
      try {
        const c = await core();
        write(c.egressHostsRemove(await read(), host, port));
      } catch (e) {
        throw toBackendError(e);
      }
    },
    request: async (request) => {
      try {
        return await run(request);
      } catch (e) {
        throw toBackendError(e);
      }
    },
  };
}
