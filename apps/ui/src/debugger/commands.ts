// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand Y: the palette entry "Debug scripts…" (a request is open).

import type { BuiltinCommandSource } from "../shell/builtin-commands";

export const debuggerCommands: BuiltinCommandSource = (app) =>
  app.collection && app.active
    ? [
        {
          id: "debugger:open",
          label: "Debug scripts…",
          run: () => app.show({ kind: "tool", tool: "debugger" }),
        },
      ]
    : [];
