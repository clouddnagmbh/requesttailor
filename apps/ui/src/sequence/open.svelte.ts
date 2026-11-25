// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Opening the sequence view with a source chosen already (the runner, the comparison, the
// palette). The view takes the preset once when it opens.

import type { SequenceSource } from "../commands/types";
import type { AppState } from "../state/app.svelte";

class PresetHolder {
  value = $state<SequenceSource | null>(null);

  /** Hands the preset out once. */
  take(): SequenceSource | null {
    const v = this.value;
    this.value = null;
    return v;
  }
}

export const presetHolder = new PresetHolder();

/** Opens "Sequence diagram" in the main area for `source` (default: the latest run). */
export function openSequence(app: AppState, source: SequenceSource = { kind: "run", run_id: 0 }): void {
  presetHolder.value = source;
  app.show({ kind: "tool", tool: "sequence" });
}
