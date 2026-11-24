// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand E: the minimal failing request, one Tauri command per method (`src-tauri/src/shrink.rs`,
// placeholders until strand E).

import type { ShrinkApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriShrink(call: Call): ShrinkApi {
  return {
    run: (request) => call("shrink_run", { request }),
    cancel: () => call("shrink_cancel"),
    predicate: (path, responseId) => call("shrink_predicate", { path, responseId }),
  };
}
