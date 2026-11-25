// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Opening the minimal-case view with a request chosen already (the failed response's action,
// the palette). The view takes the preset once when it opens.

import type { AppState } from "../state/app.svelte";

export interface ShrinkPreset {
  /** A request of the active collection. */
  path: string;
}

class PresetHolder {
  value = $state<ShrinkPreset | null>(null);

  /** Hands the preset out once. */
  take(): ShrinkPreset | null {
    const v = this.value;
    this.value = null;
    return v;
  }
}

export const presetHolder = new PresetHolder();

/** Opens "Minimal failing request" in the main area for `path` (default: the active request). */
export function openShrink(app: AppState, path: string | null = app.active): boolean {
  if (!path) return false;
  presetHolder.value = { path };
  app.show({ kind: "tool", tool: "shrink" });
  return true;
}
