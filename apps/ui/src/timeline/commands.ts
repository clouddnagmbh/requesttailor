// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the request history (wave 12, strand D): they open its view with
// `views.open(...)`.

import type { GitViews } from "../git/views.svelte";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export function timelineCommands(app: AppState, views: GitViews): Command[] {
  return [
    {
      id: "timeline-request",
      label: "History of this request",
      run: () => {
        if (app.active) views.open({ kind: "timeline", path: app.active });
        else app.error = "Open a request first to see its history.";
      },
    },
  ];
}
