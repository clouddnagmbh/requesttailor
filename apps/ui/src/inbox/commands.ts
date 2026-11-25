// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand E: palette commands of the webhook inbox (`BuiltinCommandSource`). "Inbox:
// Start" starts the receiver and opens the view; nothing starts with the app.

import type { BuiltinCommandSource } from "../shell/builtin-commands";

export const inboxCommands: BuiltinCommandSource = (app) => [
  {
    id: "inbox:open",
    label: "Inbox: Open",
    run: () => app.show({ kind: "tool", tool: "inbox" }),
  },
  {
    id: "inbox:start",
    label: "Inbox: Start",
    run: async () => {
      app.show({ kind: "tool", tool: "inbox" });
      try {
        const status = await app.backend.inbox.status();
        if (!status.running) await app.backend.inbox.start();
      } catch {
        // The view shows the reason (a port in use) when it is open; starting from there reports it.
      }
    },
  },
];
