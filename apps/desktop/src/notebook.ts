// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand W: api notebooks, one Tauri command per method (`src-tauri/src/notebook.rs`,
// placeholders until strand W).

import type { NotebookApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriNotebook(call: Call): NotebookApi {
  return {
    list: () => call("notebook_list"),
    read: (path) => call("notebook_read", { path }),
    stage: (path, notebook) => call("notebook_stage", { path, notebook }),
    run: (run) => call("notebook_run", { run }),
  };
}
