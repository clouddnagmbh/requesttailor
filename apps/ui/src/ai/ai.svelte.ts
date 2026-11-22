// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// State of the AI assistant's UI (wave 8, strand E), one per backend: whether it is set up,
// and which dialog is open. The dialogs live in `AiDialogs.svelte` (mounted once by the
// shell); the buttons in the response panel and the scripts tab only open them.

import type { Backend } from "../commands/backend";
import type { AiConfig } from "../commands/types";
import { usable } from "./apply";

export type AiView =
  | { kind: "settings" }
  | { kind: "create"; folder: string }
  | { kind: "explain"; path: string; response: number }
  | { kind: "tests"; path: string; response: number };

export class AiState {
  config = $state<AiConfig | null>(null);
  hasKey = $state(false);
  loaded = $state(false);
  view = $state<AiView | null>(null);
  /** Set up and ready: everything else of the assistant is hidden until then. */
  readonly configured = $derived(usable(this.config, this.hasKey));
  private loading: Promise<void> | null = null;

  constructor(readonly backend: Backend) {}

  /** Reads the settings once. */
  ensure(): Promise<void> {
    this.loading ??= this.reload();
    return this.loading;
  }

  async reload(): Promise<void> {
    try {
      const config = await this.backend.ai.config();
      this.hasKey = config ? await this.backend.ai.hasKey(config.provider) : false;
      this.config = config;
    } catch (e) {
      console.warn("RequestTailor: reading the AI settings failed:", e);
      this.config = null;
      this.hasKey = false;
    }
    this.loaded = true;
  }

  open(view: AiView): void {
    this.view = view;
  }

  close(): void {
    this.view = null;
  }
}

const states = new WeakMap<Backend, AiState>();

export function aiFor(backend: Backend): AiState {
  let s = states.get(backend);
  if (!s) {
    s = new AiState(backend);
    states.set(backend, s);
  }
  return s;
}
