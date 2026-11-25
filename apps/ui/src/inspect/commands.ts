// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: palette command of the JWT view (`BuiltinCommandSource`). "Inspect token…"
// opens the view with a field to paste a token into; it needs no collection.

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import JwtDialog from "./JwtDialog.svelte";

export const inspectCommands: BuiltinCommandSource = (app) => [
  {
    id: "inspect:token",
    label: "Inspect token…",
    run: () => {
      void app.showDialog({ title: "Inspect token", component: JwtDialog, width: 640, props: {} });
    },
  },
];
