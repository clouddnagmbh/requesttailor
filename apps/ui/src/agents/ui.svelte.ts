// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Which agent dialogs are open, and their palette entries (wave 8, strand C). The shell
// mounts `AgentsHost` once and spreads `agentCommands(app)` into the palette.

import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";

export const agentsUi = $state({ policy: false, audit: false });

export function agentCommands(app: AppState): Command[] {
  return [
    ...(app.collection
      ? [{ id: "agents-policy", label: "Agent access…", run: () => (agentsUi.policy = true) }]
      : []),
    { id: "agents-audit", label: "Agent audit log…", run: () => (agentsUi.audit = true) },
  ];
}
