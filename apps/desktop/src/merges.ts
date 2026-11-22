// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `MergeApi` of the desktop app (wave 12, strand B; src-tauri/src/merges.rs): the conflicts
// come from the index stages on the Rust side, `resolve` writes the files and reopens the
// collection there.

import type { ConflictChoice, MergeApi, MergeState } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriMerges(call: Call): MergeApi {
  return {
    conflicts: () => call<MergeState>("merge_conflicts"),
    resolve: (choices: ConflictChoice[]) => call<void>("merge_resolve", { choices }),
  };
}
