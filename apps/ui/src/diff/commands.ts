// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the semantic diff (wave 12, strand A): they open its view with
// `views.open(...)`.

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function diffCommands(app: AppState, views: GitViews): Command[] {
  void app;
  return [
    {
      id: "diff-working",
      label: "Changes: saved changes by field",
      run: () => views.open({ kind: "diff", scope: { kind: "working" } }),
    },
    {
      id: "diff-head",
      label: "Changes: last commit by field",
      run: () => views.open({ kind: "diff", scope: { kind: "commit", rev: "HEAD" } }),
    },
  ];
}
