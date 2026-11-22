// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of secret rescue (wave 12, strand G). Interface placeholder: none until the strand
// adds them (they open its view with `views.open(...)`).

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function rescueCommands(app: AppState, views: GitViews): Command[] {
  void app;
  void views;
  return [];
}
