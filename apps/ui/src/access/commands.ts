// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand D: the palette entry "Check access…" for the active request.

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import { openAccess } from "./open.svelte";

export const accessCommands: BuiltinCommandSource = (app) =>
  app.collection && app.active
    ? [
        {
          id: "access:open",
          label: "Check access…",
          run: () => openAccess(app),
        },
      ]
    : [];
