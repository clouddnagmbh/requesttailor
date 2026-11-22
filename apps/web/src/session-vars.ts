// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Session variables in the web app (wave 7, strand C): what scripts, actions and "set as
// variable" set, in memory per collection and environment (`""` for none), like
// `rt_app::scripts::SessionVars` on the desktop. Every send and preview gets them in its
// runtime layer, before the caller's own `vars` (which win); after a send the report's
// `var_changes` are kept, whatever their scope (files are never written). Values are as
// reported, so a captured secret stays masked.

import type { SendReport, VarChange } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

export interface WebSessionVars {
  sessionVars(environment: string): Promise<VarChange[]>;
  setSessionVar(environment: string, change: VarChange): Promise<void>;
  clearSessionVars(environment: string): Promise<void>;
}

/** The session's variables of every collection, for the send path and `ScriptsApi`. */
export class SessionStore {
  private readonly byKey = new Map<string, VarChange[]>();

  private key(collection: string, environment: string | null): string {
    return JSON.stringify([collection, environment ?? ""]);
  }

  list(collection: string, environment: string | null): VarChange[] {
    return [...(this.byKey.get(this.key(collection, environment)) ?? [])];
  }

  /** A value replaces the variable (moved to the end), `null` removes it. */
  apply(collection: string, environment: string | null, change: VarChange): void {
    const key = this.key(collection, environment);
    const list = (this.byKey.get(key) ?? []).filter((c) => c.name !== change.name);
    if (change.value !== null) list.push({ ...change });
    if (list.length > 0) this.byKey.set(key, list);
    else this.byKey.delete(key);
  }

  clear(collection: string, environment: string | null): void {
    this.byKey.delete(this.key(collection, environment));
  }

  /** `vars` of a send or preview: the session's first, then the caller's own (they win). */
  runtime(collection: string, environment: string | null, own: [string, string][]): [string, string][] {
    const session = this.list(collection, environment).flatMap((c): [string, string][] =>
      c.value === null ? [] : [[c.name, c.value]],
    );
    return [...session, ...own];
  }

  /** Keeps what a send set for its environment. */
  keep(collection: string, report: SendReport): void {
    for (const change of report.var_changes ?? []) this.apply(collection, report.environment, change);
  }
}

/** `ScriptsApi`'s session part for the open collection (none open: empty, changes dropped). */
export function webSessionVars(host: Pick<WebHost, "session">, store: SessionStore): WebSessionVars {
  const collection = (): string | null => {
    try {
      return host.session().id;
    } catch {
      return null;
    }
  };
  return {
    sessionVars: async (environment) => {
      const id = collection();
      return id === null ? [] : store.list(id, environment);
    },
    setSessionVar: async (environment, change) => {
      const id = collection();
      if (id !== null) store.apply(id, environment, change);
    },
    clearSessionVars: async (environment) => {
      const id = collection();
      if (id !== null) store.clear(id, environment);
    },
  };
}
