// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Session variables of the open collection and the active environment, shared by the views
// that show or set them (wave 7, strand C). What scripts and actions set comes back from the
// shell after every send (`app.sendCount`); "set as variable" goes through `set`. `revision`
// changes on every change, so views that depend on the variables (the editor's preview)
// can re-read.

import type { VarChange } from "../commands/types";
import type { AppState } from "../state/app.svelte";

export class SessionVarsState {
  list = $state<VarChange[]>([]);
  /** Bumped after every change made here and every refresh that found a change. */
  revision = $state(0);
  private seq = 0;

  constructor(private readonly app: AppState) {}

  private get environment(): string {
    return this.app.environment ?? "";
  }

  async refresh(): Promise<void> {
    const seq = ++this.seq;
    try {
      const next = this.app.collection ? await this.app.backend.scripts.sessionVars(this.environment) : [];
      if (seq !== this.seq) return;
      if (JSON.stringify(next) !== JSON.stringify(this.list)) {
        this.list = next;
        this.revision++;
      }
    } catch {
      if (seq === this.seq) this.list = [];
    }
  }

  async set(change: VarChange): Promise<void> {
    await this.app.backend.scripts.setSessionVar(this.environment, change);
    this.revision++;
    await this.refresh();
  }

  async remove(name: string): Promise<void> {
    await this.set({ name, value: null, scope: "runtime" });
  }

  async clear(): Promise<void> {
    await this.app.backend.scripts.clearSessionVars(this.environment);
    this.revision++;
    await this.refresh();
  }
}

const states = new WeakMap<AppState, SessionVarsState>();

/** The one session-variable state of `app`. Call `track` from a component to keep it fresh. */
export function sessionVars(app: AppState): SessionVarsState {
  let s = states.get(app);
  if (!s) states.set(app, (s = new SessionVarsState(app)));
  return s;
}

/** Re-reads after every send, environment change and collection load. Use inside `$effect`. */
export function track(app: AppState, s: SessionVarsState): void {
  void app.sendCount;
  void app.environment;
  void app.loadCount;
  void s.refresh();
}
