// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the semantic merge (wave 12, strand B): open the merge view, which lists
// the field conflicts of the merge in progress.

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function mergeCommands(app: AppState, views: GitViews): Command[] {
  if (!app.collection) return [];
  return [{ id: "merge-conflicts", label: "Resolve merge conflicts", run: () => views.open({ kind: "merge" }) }];
}
