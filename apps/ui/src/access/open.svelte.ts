// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Opening the access-check view with a request chosen already (the palette). The view takes the
// preset once when it opens.

import type { AppState } from "../state/app.svelte";

export interface AccessPreset {
  /** A request of the active collection. */
  path: string;
}

class PresetHolder {
  value = $state<AccessPreset | null>(null);

  /** Hands the preset out once. */
  take(): AccessPreset | null {
    const v = this.value;
    this.value = null;
    return v;
  }
}

export const presetHolder = new PresetHolder();

/** Opens "Check access" in the main area for `path` (default: the active request). */
export function openAccess(app: AppState, path: string | null = app.active): boolean {
  if (!path) return false;
  presetHolder.value = { path };
  app.show({ kind: "tool", tool: "access" });
  return true;
}
