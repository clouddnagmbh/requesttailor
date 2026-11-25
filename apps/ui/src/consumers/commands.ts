// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand F: the palette entry "Verify consumers…" (the collection lists consumers).

import type { BuiltinCommandSource } from "../shell/builtin-commands";

export const consumersCommands: BuiltinCommandSource = (app) =>
  app.collection
    ? [
        {
          id: "consumers:open",
          label: "Verify consumers…",
          run: () => app.show({ kind: "tool", tool: "consumers" }),
        },
      ]
    : [];
