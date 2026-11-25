// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Opening the notebook view with a notebook chosen already. The view takes the preset once.

import type { AppState } from "../state/app.svelte";

class PresetHolder {
  path = $state<string | null>(null);

  take(): string | null {
    const v = this.path;
    this.path = null;
    return v;
  }
}

export const presetHolder = new PresetHolder();

/** Opens the notebooks view in the main area (`path`: a notebook to show first). */
export function openNotebook(app: AppState, path: string | null = null): boolean {
  presetHolder.path = path;
  app.show({ kind: "tool", tool: "notebook" });
  return true;
}
