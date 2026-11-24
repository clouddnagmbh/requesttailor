// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface: palette commands of the built-in parts, one source per part
// (`<part>/commands.ts`), so strands add commands without touching `shell/commands.ts`.

import type { Command } from "./commands";
import type { AppState } from "../state/app.svelte";
import { catalogueCommands } from "../catalogue/commands";
import { envcompareCommands } from "../envcompare/commands";
import { inboxCommands } from "../inbox/commands";
import { inspectCommands } from "../inspect/commands";
import { lintCommands } from "../lint/commands";
import { pagingCommands } from "../paging/commands";
import { ratelimitCommands } from "../ratelimit/commands";
import { viewsCommands } from "../views/commands";

/** What a part needs to offer commands: the app state. Called reactively: cheap, synchronous,
 *  no `$state` writes. */
export type BuiltinCommandSource = (app: AppState) => Command[];

const SOURCES: BuiltinCommandSource[] = [
  envcompareCommands,
  inspectCommands,
  pagingCommands,
  ratelimitCommands,
  inboxCommands,
  lintCommands,
  viewsCommands,
  catalogueCommands,
];

/** The wave 18 commands of every part, in a fixed order. */
export function builtinCommands(app: AppState): Command[] {
  return SOURCES.flatMap((source) => source(app));
}
