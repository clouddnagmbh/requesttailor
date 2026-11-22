// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `MergeApi` of the desktop app (wave 12, strand B; src-tauri/src/merges.rs). Interface
// placeholder: the Tauri commands are registered and refuse; the strand calls them here.

import { stubMerges, type MergeApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriMerges(call: Call): MergeApi {
  void call;
  return stubMerges();
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. The strand empties the list as it maps them. */
export const PENDING: readonly Command[] = [
  "merge_conflicts",
  "merge_resolve",
];
