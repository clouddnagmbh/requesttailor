// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12 interface: the palette entries of all Git-for-APIs strands, in one list for the
// palette (AppShell) and the Git view's "Tools" section (GitPanel). Each strand edits only its
// own `commands.ts`. Wave 14 (strand G) adds the Git wizards (`wizards/commands.ts`).

import { compareCommands } from "../compare/commands";
import { contractCommands } from "../contract/commands";
import { diffCommands } from "../diff/commands";
import { mergeCommands } from "../merge/commands";
import { rescueCommands } from "../rescue/commands";
import { reviewCommands } from "../review/commands";
import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";
import { timelineCommands } from "../timeline/commands";
import { historyCommands } from "./history/commands";
import type { GitViews } from "./views.svelte";
import { gitWizardCommands } from "./wizards/commands";

export function gitApiCommands(app: AppState, views: GitViews): Command[] {
  // A set with only repository roots has no collection: the generic Git commands still work on
  // the active repository (history, wizards, pull requests); the API-specific ones need a collection.
  if (!app.collection) {
    return app.activeRepository ? [...reviewCommands(app, views), ...historyCommands(app, views), ...gitWizardCommands(app, views)] : [];
  }
  return [
    ...diffCommands(app, views),
    ...mergeCommands(app, views),
    ...compareCommands(app, views),
    ...timelineCommands(app, views),
    ...reviewCommands(app, views),
    ...contractCommands(app, views),
    ...rescueCommands(app, views),
    ...historyCommands(app, views),
    ...gitWizardCommands(app, views),
  ];
}
