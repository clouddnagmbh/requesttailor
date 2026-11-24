// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand C: paging, one Tauri command per method (`src-tauri/src/paging.rs`,
// placeholders until strand C).

import type { PagingApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriPaging(call: Call): PagingApi {
  return {
    rule: (path) => call("paging_rule", { path }),
    setRule: (path, spec, scope) => call("paging_set_rule", { path, spec, scope }),
    run: (run) => call("paging_run", { run }),
  };
}
