// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand C: the palette entry "Test-data journal".

import type { BuiltinCommandSource } from "../shell/builtin-commands";

export const journalCommands: BuiltinCommandSource = (app) =>
  app.collection
    ? [
        {
          id: "journal:open",
          label: "Test-data journal",
          run: () => app.show({ kind: "tool", tool: "journal" }),
        },
      ]
    : [];
