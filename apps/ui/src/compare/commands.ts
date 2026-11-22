// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of branch compare and bisect (wave 12, strand C). They open the compare and
// bisect views (`GitViews`); the revisions are typed there.

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function compareCommands(app: AppState, views: GitViews): Command[] {
  void app;
  return [
    {
      id: "git:compare",
      label: "Compare revisions…",
      run: () => views.open({ kind: "compare" }),
    },
    {
      id: "git:bisect",
      label: "Bisect: find the commit that broke a test…",
      run: () => views.open({ kind: "bisect" }),
    },
  ];
}
