// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand E: the palette entry "Shrink failing request…" for the active request. The
// action on a failed response (`ShrinkButton.svelte`) calls `openShrink` as well.

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import { openShrink } from "./open.svelte";

export const shrinkCommands: BuiltinCommandSource = (app) =>
  app.collection && app.active
    ? [
        {
          id: "shrink:open",
          label: "Shrink failing request…",
          run: () => openShrink(app),
        },
      ]
    : [];
