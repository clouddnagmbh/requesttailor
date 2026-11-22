// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: the palette entries of all Git-for-APIs strands, in one list for the
// palette (AppShell) and the Git view's "Tools" section (GitPanel). Each strand edits only its
// own `commands.ts`.

import { compareCommands } from "../compare/commands";
import { contractCommands } from "../contract/commands";
import { diffCommands } from "../diff/commands";
import { mergeCommands } from "../merge/commands";
import { rescueCommands } from "../rescue/commands";
import { reviewCommands } from "../review/commands";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";
import { timelineCommands } from "../timeline/commands";
import type { GitViews } from "./views.svelte";

export function gitApiCommands(app: AppState, views: GitViews): Command[] {
  if (!app.collection) return [];
  return [
    ...diffCommands(app, views),
    ...mergeCommands(app, views),
    ...compareCommands(app, views),
    ...timelineCommands(app, views),
    ...reviewCommands(app, views),
    ...contractCommands(app, views),
    ...rescueCommands(app, views),
  ];
}
