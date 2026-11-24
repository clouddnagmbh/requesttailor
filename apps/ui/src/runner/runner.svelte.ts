// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// State of the collection runner panel (Svelte 5 runes): what to run (a folder or the whole
// collection), the configuration, and the results of the current or last run. One per
// shell; the run goes on while the panel is closed. Owner: wave 7, strand D.
//
// Events can arrive by the thousand: they go into a plain `RunResults` and the view is told
// once per animation frame (`version`), so long runs stay smooth.

import type { ImportSource } from "../commands/backend";
import type { DataRow, RunEvent } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { planOf, RunResults } from "./model";

export interface RunTarget {
  /** The collection it runs in (wave 14); `undefined`: the active one when it starts. */
  collection?: string;
  /** Folder path, `null`: the whole collection. */
  folder: string | null;
  /** Display name: the folder's or the collection's. */
  name: string;
}

const frame = (f: () => void) =>
  typeof requestAnimationFrame === "function" ? requestAnimationFrame(f) : setTimeout(f, 16);

export class RunnerState {
  /** The panel shows this target; `null`: closed. */
  target = $state<RunTarget | null>(null);
  iterations = $state(1);
  delayMs = $state(0);
  bail = $state(false);
  data = $state.raw<DataRow[] | null>(null);
  dataName = $state<string | null>(null);
  dataError = $state<string | null>(null);
  running = $state(false);
  /** Target of the results shown (the last run). */
  ran = $state<RunTarget | null>(null);
  /** Bumped when `results` changed; the view reads it to re-render. */
  version = $state(0);
  error = $state<string | null>(null);
  results = new RunResults();

  /** Asked once before a run starts (wave 17: the production question); `false` cancels. */
  beforeStart: ((target: RunTarget) => Promise<boolean>) | null = null;

  private scheduled = false;

  constructor(private readonly app: AppState) {}

  /** Opens the panel for `folder` (`null`: the collection). Results of a finished run stay
   *  until the next run. */
  open(target: RunTarget): void {
    this.target = target;
  }

  close(): void {
    this.target = null;
  }

  async loadData(file: ImportSource): Promise<void> {
    this.dataError = null;
    try {
      const rows = await this.app.backend.runner.parseData(file);
      this.data = rows;
      this.dataName = file.name;
    } catch (e) {
      this.data = null;
      this.dataName = null;
      this.dataError = e instanceof Error ? e.message : String(e);
    }
  }

  clearData(): void {
    this.data = null;
    this.dataName = null;
    this.dataError = null;
  }

  /** Runs the open target with the current configuration and the active environment. */
  async start(): Promise<void> {
    const target = this.target;
    if (!target || this.running) return;
    if (this.beforeStart && !(await this.beforeStart(target).catch(() => true))) return;
    if (this.running) return;
    this.running = true;
    this.error = null;
    this.ran = target;
    this.results = new RunResults();
    this.version++;
    try {
      const backend = this.app.backend;
      // Wave 14: the runner acts on the active collection; the target's comes first.
      if (target.collection && this.app.activeCollection !== target.collection) await this.app.activate(target.collection);
      const requests = await backend.runner.plan(target.folder);
      const plan = planOf(requests, {
        iterations: this.iterations,
        delayMs: this.delayMs,
        bail: this.bail,
        data: this.data,
      });
      const results = this.results;
      const summary = await backend.runner.run(
        plan,
        { environment: this.app.environment, vars: [] },
        (event: RunEvent) => {
          results.apply(event);
          this.changed();
        },
      );
      // A backend that resolves without a `finished` event still ends the run.
      if (!results.summary) results.summary = summary;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.running = false;
      this.version++;
    }
  }

  async cancel(): Promise<void> {
    if (!this.running) return;
    try {
      await this.app.backend.runner.cancel();
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  private changed(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    frame(() => {
      this.scheduled = false;
      this.version++;
    });
  }
}
