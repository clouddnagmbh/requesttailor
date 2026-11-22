// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ScriptsApi` in the mock backend (wave 7). Trust: strand A; session variables: strand C.
// The mock runs no JavaScript; its send reports no tests (`?backend=mock` UI tests set
// results through `MockBackend` where they need them).

import type { ScriptsApi } from "./backend";
import type { VarChange } from "./types";

export interface MockScripts extends ScriptsApi {
  /** Whether the open collection has scripts; `?backend=mock&scripts=1` starts with `true`
   *  (UI tests of the trust banner). */
  hasScripts: boolean;
}

/** `?scripts=1` in the page URL (none outside a browser). */
function scriptsParam(): boolean {
  try {
    return new URLSearchParams(globalThis.location?.search ?? "").get("scripts") === "1";
  } catch {
    return false;
  }
}

export function mockScripts(): MockScripts {
  let trusted = false;
  const session = new Map<string, VarChange[]>();
  return {
    hasScripts: scriptsParam(),
    async trust() {
      return { has_scripts: this.hasScripts, trusted };
    },
    async setTrusted(on) {
      trusted = on;
    },
    async sessionVars(environment) {
      return [...(session.get(environment) ?? [])];
    },
    async setSessionVar(environment, change) {
      const list = (session.get(environment) ?? []).filter((c) => c.name !== change.name);
      if (change.value !== null) list.push(change);
      session.set(environment, list);
    },
    async clearSessionVars(environment) {
      session.delete(environment);
    },
  };
}
