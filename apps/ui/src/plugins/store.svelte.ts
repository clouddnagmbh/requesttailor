// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The installed plugins as reactive state, one per backend: the dialog changes them, request
// and response tabs and the import dialog read them. Owner: wave 5, strand A.

import type { Backend } from "../commands/backend";
import type { PluginInfo } from "../commands/types";

export class PluginsState {
  list = $state<PluginInfo[]>([]);
  /** The last error of `refresh` (no plugin host, for example), or `null`. */
  error = $state<string | null>(null);
  private loaded: Promise<void> | null = null;

  constructor(private readonly backend: Backend) {}

  /** Re-reads the list from the backend. */
  async refresh(): Promise<void> {
    try {
      this.list = await this.backend.plugins.list();
      this.error = null;
    } catch (e) {
      this.list = [];
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  /** Reads the list once (first use). */
  ensure(): Promise<void> {
    return (this.loaded ??= this.refresh());
  }
}

const states = new WeakMap<Backend, PluginsState>();

/** The plugin state of `backend`, loaded on first use unless `load` is false. Components
 *  that are mounted from the start (dialogs) pass `false` and refresh when they open: listing
 *  plugins loads the web core, which must not happen before a collection is opened. */
export function pluginsState(backend: Backend, load = true): PluginsState {
  let s = states.get(backend);
  if (!s) {
    s = new PluginsState(backend);
    states.set(backend, s);
  }
  if (load) void s.ensure();
  return s;
}
