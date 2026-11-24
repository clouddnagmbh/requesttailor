// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface: palette commands of the built-in parts, one source per part
// (`<part>/commands.ts`), so strands add commands without touching `shell/commands.ts`.

import type { Command } from "./commands";
import type { AppState } from "../state/app.svelte";
import { accessCommands } from "../access/commands";
import { catalogueCommands } from "../catalogue/commands";
import { consumersCommands } from "../consumers/commands";
import { debuggerCommands } from "../debugger/commands";
import { envcompareCommands } from "../envcompare/commands";
import { inboxCommands } from "../inbox/commands";
import { inspectCommands } from "../inspect/commands";
import { journalCommands } from "../journal/commands";
import { lintCommands } from "../lint/commands";
import { notebookCommands } from "../notebook/commands";
import { securityCommands } from "../security/commands";
import { sequenceCommands } from "../sequence/commands";
import { shrinkCommands } from "../shrink/commands";
import { traceCommands } from "../trace/commands";
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
  // Wave 19.
  traceCommands,
  accessCommands,
  shrinkCommands,
  consumersCommands,
  journalCommands,
  sequenceCommands,
  securityCommands,
  notebookCommands,
  debuggerCommands,
];

/** The wave 18 and 19 commands of every part, in a fixed order. */
export function builtinCommands(app: AppState): Command[] {
  return SOURCES.flatMap((source) => source(app));
}
