// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of secret rescue (wave 12, strand G): open the wizard (`RescueWizard`), which
// scans the history only when asked and rewrites only after an explicit confirmation.

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function rescueCommands(app: AppState, views: GitViews): Command[] {
  void app;
  return [
    {
      id: "git:rescue",
      label: "Scan history for secrets…",
      run: () => views.open({ kind: "rescue" }),
    },
  ];
}
