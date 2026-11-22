// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompanionApi` of the desktop app (src-tauri/src/companions.rs, wave 13): companion files of
// the open collection, staged in the Rust workspace until `save`.

import type { CompanionApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriCompanions(call: Call): CompanionApi {
  return {
    list: (prefix) => call("companion_list", { prefix }),
    read: (path) => call("companion_read", { path }),
    stage: (path, text) => call("companion_stage", { path, text }),
  };
}
