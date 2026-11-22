// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the contract check (wave 12, strand F). Interface placeholder: none until the strand
// adds them (they open its view with `views.open(...)`).

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function contractCommands(app: AppState, views: GitViews): Command[] {
  void app;
  void views;
  return [];
}
