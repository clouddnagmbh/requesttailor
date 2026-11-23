// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// State of the AI assistant's UI (wave 8, strand E), one per backend: whether it is set up,
// and which dialog is open. The dialogs live in `AiDialogs.svelte` (mounted once by the
// shell); the buttons in the response panel and the scripts tab only open them.

import type { Backend } from "../commands/backend";
import type { AiCollectionPolicy, AiConfig, AiSettings } from "../commands/types";
import { usable } from "./apply";

export type AiView =
  | { kind: "settings" }
  | { kind: "create"; folder: string; /** Wave 16: what was typed, when the dialog opens again after a failed try. */ text?: string; mode?: "describe" | "snippet" }
  | { kind: "explain"; path: string; response: number }
  | { kind: "tests"; path: string; response: number };

export class AiState {
  config = $state<AiConfig | null>(null);
  /** Wave 16: profiles, roles and the preview mode; `null` until set up. */
  settings = $state<AiSettings | null>(null);
  hasKey = $state(false);
  loaded = $state(false);
  view = $state<AiView | null>(null);
  /** Wave 16: whether the active collection allows the assistant; `null` until read. */
  policy = $state<AiCollectionPolicy | null>(null);
  /** Wave 16: what a running task of the app's own is doing ("Asking the model…"), or `null`. */
  working = $state<string | null>(null);
  /** Set up and ready: everything else of the assistant is hidden until then. */
  readonly configured = $derived(usable(this.config, this.hasKey));
  /** Wave 16: set up, and not turned off for this collection (file or device). */
  readonly allowed = $derived(!this.policy || (!this.policy.file_off && !this.policy.device_off));
  /** Wave 16: set up and usable: the entries of the new tasks show only then. */
  readonly ready = $derived(this.configured && this.allowed);
  /** Wave 16: set up but off for this collection: the entries show disabled, with the reason. */
  readonly off = $derived(this.configured && !this.allowed);
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
      const settings = await this.backend.ai.settings();
      // The default profile's own key, else its provider's (what a call uses).
      const profile = settings?.profiles.find((p) => p.id === settings.default_profile) ?? settings?.profiles[0];
      this.hasKey = config
        ? ((profile ? await this.backend.ai.hasProfileKey(profile.id) : false) || (await this.backend.ai.hasKey(config.provider)))
        : false;
      this.settings = settings;
      this.config = config;
    } catch (e) {
      console.warn("RequestTailor: reading the AI settings failed:", e);
      this.config = null;
      this.settings = null;
      this.hasKey = false;
    }
    this.loaded = true;
  }

  /** Wave 16: reads the policy of the active collection (it changes with the collection and
   *  with the device switch). A failure counts as allowed: the call itself refuses. */
  async refreshPolicy(): Promise<void> {
    try {
      this.policy = await this.backend.ai.policy();
    } catch {
      this.policy = null;
    }
  }

  open(view: AiView): void {
    this.view = view;
  }

  close(): void {
    const settings = this.view?.kind === "settings";
    this.view = null;
    // The settings dialog may have turned the assistant off or on for this collection.
    if (settings) void this.refreshPolicy();
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
