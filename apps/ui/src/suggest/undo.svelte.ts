// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The last applied suggestion and how to take it back (wave 16, strand C). One per app; the
// toast (`SuggestHost.svelte`) shows it and the palette offers "Undo suggestion".

import type { AppState } from "../state/app.svelte";

export interface AppliedSuggestion {
  title: string;
  subtitle: string;
  /** `null`: cannot be reverted; the toast says where to discard it. */
  undo: (() => Promise<boolean>) | null;
  seq: number;
}

export class SuggestUndo {
  last = $state<AppliedSuggestion | null>(null);
  /** The outcome of the last undo, for the toast. */
  message = $state<string | null>(null);
  private seq = 0;

  push(entry: Omit<AppliedSuggestion, "seq">): void {
    this.message = null;
    this.last = { ...entry, seq: ++this.seq };
  }

  /** Reverts the last suggestion. `true` if it was put back. */
  async run(): Promise<boolean> {
    const entry = this.last;
    if (!entry?.undo) return false;
    let done = false;
    try {
      done = await entry.undo();
      this.message = done ? "Suggestion undone." : "Not undone: it was changed after it was applied.";
    } catch (e) {
      this.message = e instanceof Error ? e.message : String(e);
    }
    if (this.last === entry) this.last = null;
    return done;
  }

  dismiss(): void {
    this.last = null;
    this.message = null;
  }
}

const stores = new WeakMap<AppState, SuggestUndo>();

export function suggestUndoFor(app: AppState): SuggestUndo {
  let s = stores.get(app);
  if (!s) stores.set(app, (s = new SuggestUndo()));
  return s;
}
