// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand A: repositories in the collection set, one Tauri command per method
// (`src-tauri/src/repositories.rs`, placeholders until strand A).

import type { RepositoryApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriRepositories(call: Call): RepositoryApi {
  return {
    open: () => call("repositories_open"),
    activate: (id) => call("repositories_activate", { id }),
    summary: (id) => call("repositories_summary", { id }),
    clone: (request) => call("repositories_clone", { request }),
    init: () => call("repositories_init"),
  };
}
