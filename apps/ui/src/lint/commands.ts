// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand F: palette command of the lint (`BuiltinCommandSource`). Only offered when a
// collection is open.

import type { BuiltinCommandSource } from "../shell/builtin-commands";

export const lintCommands: BuiltinCommandSource = (app) =>
  app.collection
    ? [
        {
          id: "lint:run",
          label: "Lint collection",
          run: () => app.show({ kind: "tool", tool: "lint" }),
        },
      ]
    : [];
