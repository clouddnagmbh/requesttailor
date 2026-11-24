// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The reactive side of the production protection: the collection's agent policy, whether the
// active environment is production, and the question "Send POST to prod?" while one is open.
// One per app (`getProduction`). The shell's `ProductionFrame` draws it; the gates it hands
// out sit in front of `AppState.send` and the runner's start.

import type { AgentPolicy } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { type ConfirmInfo, confirmEnabled, isProduction, shouldConfirm } from "./production";

/** What the open question is about: one request, or a whole run. */
export interface PendingConfirm extends ConfirmInfo {
  kind: "send" | "run";
}

export class ProductionState {
  policy = $state<AgentPolicy | null>(null);
  /** The open question, or `null`. */
  pending = $state<PendingConfirm | null>(null);

  private resolve: ((yes: boolean) => void) | null = null;

  constructor(private readonly app: AppState) {}

  /** The active environment is a production system. */
  get active(): boolean {
    return isProduction(this.policy, this.app.environment);
  }

  /** Reads the active collection's policy (after open, reload and edits). */
  async refresh(): Promise<void> {
    if (!this.app.collection) {
      this.policy = null;
      return;
    }
    try {
      this.policy = await this.app.backend.agents.policy();
    } catch {
      this.policy = null;
    }
  }

  /** `AppState.sendConfirm`: `false` cancels the send. Asks for a writing request in
   *  production; an error from the check lets the send through (the core still decides). */
  async gate(path: string): Promise<boolean> {
    const environment = this.app.environment;
    if (!environment || !this.active || !confirmEnabled()) return true;
    let effect;
    try {
      effect = (await this.app.backend.agents.check(path, environment)).effect;
    } catch {
      return true;
    }
    if (!shouldConfirm({ production: true, effect, enabled: true })) return true;
    const spec = await this.app.backend.getRequest(path).catch(() => null);
    return this.ask({ kind: "send", method: spec?.method ?? "?", url: spec?.url ?? path, environment });
  }

  /** `RunnerState.beforeStart`: once per run, whatever its requests are. */
  async gateRun(what: string): Promise<boolean> {
    const environment = this.app.environment;
    if (!environment || !this.active || !confirmEnabled()) return true;
    return this.ask({ kind: "run", method: "RUN", url: what, environment });
  }

  /** Answers the open question (Enter: yes, Escape: no). */
  answer(yes: boolean): void {
    const resolve = this.resolve;
    this.resolve = null;
    this.pending = null;
    resolve?.(yes);
  }

  private ask(info: PendingConfirm): Promise<boolean> {
    // A second question while one is open cancels the first.
    this.answer(false);
    this.pending = info;
    return new Promise((resolve) => (this.resolve = resolve));
  }
}

const states = new WeakMap<AppState, ProductionState>();

export function getProduction(app: AppState): ProductionState {
  let state = states.get(app);
  if (!state) {
    state = new ProductionState(app);
    states.set(app, state);
  }
  return state;
}
