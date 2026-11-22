// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Session variables in the web app (wave 7, strand C owns this file): what scripts, actions
// and "set as variable" set, in memory per collection and environment; they go into the
// runtime layer of later sends. Interface stub: kept, not yet used by sends.

import type { VarChange } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

export interface WebSessionVars {
  sessionVars(environment: string): Promise<VarChange[]>;
  setSessionVar(environment: string, change: VarChange): Promise<void>;
  clearSessionVars(environment: string): Promise<void>;
}

export function webSessionVars(_host: Pick<WebHost, "session">): WebSessionVars {
  const byEnvironment = new Map<string, VarChange[]>();
  return {
    sessionVars: async (environment) => [...(byEnvironment.get(environment) ?? [])],
    setSessionVar: async (environment, change) => {
      const list = (byEnvironment.get(environment) ?? []).filter((c) => c.name !== change.name);
      if (change.value !== null) list.push(change);
      byEnvironment.set(environment, list);
    },
    clearSessionVars: async (environment) => {
      byEnvironment.delete(environment);
    },
  };
}
