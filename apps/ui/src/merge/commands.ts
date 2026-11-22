// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the semantic merge (wave 12, strand B). Interface placeholder: none until the strand
// adds them (they open its view with `views.open(...)`).

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function mergeCommands(app: AppState, views: GitViews): Command[] {
  void app;
  void views;
  return [];
}
