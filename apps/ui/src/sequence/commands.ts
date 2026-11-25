// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand G: palette commands of sequence diagrams.

import type { BuiltinCommandSource } from "../shell/builtin-commands";
import { openSequence } from "./open.svelte";

export const sequenceCommands: BuiltinCommandSource = (app) => {
  const out = [
    {
      id: "sequence:run",
      label: "Sequence diagram: latest run…",
      run: () => openSequence(app, { kind: "run", run_id: 0 }),
    },
    {
      id: "sequence:session",
      label: "Sequence diagram: this session…",
      run: () => openSequence(app, { kind: "session" }),
    },
  ];
  if (app.collection && app.active) {
    const path = app.active;
    out.push({
      id: "sequence:pages",
      label: "Sequence diagram: pages of this request…",
      run: () => openSequence(app, { kind: "pages", request_path: path }),
    });
  }
  return out;
};
