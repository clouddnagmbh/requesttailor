// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DiffApi` of the desktop app (wave 12, strand A; src-tauri/src/diffs.rs). Interface
// placeholder: the Tauri commands are registered and refuse; the strand calls them here.

import { stubDiffs, type DiffApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriDiffs(call: Call): DiffApi {
  void call;
  return stubDiffs();
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. The strand empties the list as it maps them. */
export const PENDING: readonly Command[] = [
  "diff_working",
  "diff_commit",
  "diff_between",
];
