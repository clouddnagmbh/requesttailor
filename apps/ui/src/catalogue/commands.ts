// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand I: the palette command of the plugin catalogue (`BuiltinCommandSource`).
// Opening it reads nothing from the network.

import type { BuiltinCommandSource } from "../shell/builtin-commands";

export const catalogueCommands: BuiltinCommandSource = (app) => [
  {
    id: "catalogue:open",
    label: "Plugin catalogue",
    run: () => app.show({ kind: "tool", tool: "catalogue" }),
  },
];
