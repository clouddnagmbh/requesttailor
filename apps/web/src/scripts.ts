// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Script trust in the web app (wave 7, strand A owns this file): which collections the user
// trusts to run scripts, kept in localStorage per collection id. Storage that is missing or
// refuses (private mode, blocked site data) falls back to memory for this page. Every send
// passes `scripts: true | false` to rt-wasm (`crates/rt-wasm/src/scripts_js.rs`): `true` runs
// the sandbox, `false` sends without scripts and notes why.

import type { ScriptTrust } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

export const TRUST_KEY = "requesttailor.trusted-scripts";

export interface WebScriptTrust {
  trust(): Promise<ScriptTrust>;
  setTrusted(trusted: boolean): Promise<void>;
  /** `services.scripts` of a send from `collectionId`. */
  forSend(collectionId: string): boolean;
}

/** The part of `Storage` this module uses. */
export type TrustStorage = Pick<Storage, "getItem" | "setItem">;

function browserStorage(): TrustStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export function webScriptTrust(
  host: Pick<WebHost, "session">,
  storage: TrustStorage | null = browserStorage(),
): WebScriptTrust {
  // What this page knows; storage is read on every question (another tab may have trusted).
  const memory = new Set<string>();

  function load(): Set<string> {
    const ids = new Set(memory);
    try {
      const raw = storage?.getItem(TRUST_KEY);
      const parsed: unknown = raw ? JSON.parse(raw) : [];
      if (Array.isArray(parsed)) {
        for (const id of parsed) if (typeof id === "string") ids.add(id);
      }
    } catch {
      // Unreadable or broken: what memory knows.
    }
    return ids;
  }

  function isTrusted(id: string): boolean {
    return load().has(id);
  }

  return {
    async trust() {
      const s = host.session();
      return { has_scripts: s.ws.hasScripts(), trusted: isTrusted(s.id) };
    },
    async setTrusted(trusted) {
      const { id } = host.session();
      const ids = load();
      if (trusted) {
        ids.add(id);
        memory.add(id);
      } else {
        ids.delete(id);
        memory.delete(id);
      }
      try {
        storage?.setItem(TRUST_KEY, JSON.stringify([...ids].sort()));
      } catch {
        // Kept in memory for this page.
      }
    },
    forSend: (collectionId) => isTrusted(collectionId),
  };
}
