// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Opening the environment comparison with something chosen already: the folder of a tree node,
// or the environments of the environment manager. The view takes the preset once when it opens.

import type { AppState } from "../state/app.svelte";

export interface EnvComparePreset {
  /** A folder of the active collection (`""`: all of it). */
  folder?: string;
  /** Environments to compare, the first is the reference. */
  environments?: string[];
}

class PresetHolder {
  value = $state<EnvComparePreset | null>(null);

  /** Hands the preset out once. */
  take(): EnvComparePreset | null {
    const v = this.value;
    this.value = null;
    return v;
  }
}

export const presetHolder = new PresetHolder();

/** Opens "Compare environments" in the main area. */
export function openEnvCompare(app: AppState, preset: EnvComparePreset = {}): boolean {
  presetHolder.value = preset;
  app.show({ kind: "tool", tool: "env-compare" });
  return true;
}
