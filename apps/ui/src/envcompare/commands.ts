// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand A: the palette entry "Compare environments…". The context menus of the
// collection and folder nodes and the environment manager call `openEnvCompare` (./open.svelte).

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import { openEnvCompare } from "./open.svelte";

export const envcompareCommands: BuiltinCommandSource = (app) =>
  app.collection
    ? [
        {
          id: "envcompare:open",
          label: "Compare environments…",
          run: () => openEnvCompare(app),
        },
      ]
    : [];
