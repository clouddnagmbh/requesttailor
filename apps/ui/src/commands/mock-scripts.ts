// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ScriptsApi` in the mock backend (wave 7). Trust: strand A; session variables: strand C.
// The mock runs no JavaScript; its send reports no tests (`?backend=mock` UI tests set
// results through `MockBackend` where they need them).

import type { ScriptsApi } from "./backend";
import type { VarChange } from "./types";

export interface MockScripts extends ScriptsApi {
  /** Set by the mock backend when a collection is opened. */
  hasScripts: boolean;
}

export function mockScripts(): MockScripts {
  let trusted = false;
  const session = new Map<string, VarChange[]>();
  return {
    hasScripts: false,
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
