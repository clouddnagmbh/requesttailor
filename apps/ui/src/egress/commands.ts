// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: the palette entry "Egress hosts…", the list of hosts the user allowed for
// extensions on this device (with a button to remove each).

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import HostsDialog from "./HostsDialog.svelte";

export const egressCommands: BuiltinCommandSource = (app) => [
  {
    id: "egress:hosts",
    label: "Egress hosts…",
    run: () => void app.showDialog({ title: "Egress hosts", component: HostsDialog, width: 520 }),
  },
];
