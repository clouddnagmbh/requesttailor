// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompareApi` of the desktop app (wave 12, strand C; src-tauri/src/compare.rs). Interface
// placeholder: the Tauri commands are registered and refuse; the strand calls them here.

import { stubCompare, type CompareApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriCompare(call: Call): CompareApi {
  void call;
  return stubCompare();
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. The strand empties the list as it maps them. */
export const PENDING: readonly Command[] = [
  "compare_run",
  "compare_revisions",
  "compare_bisect",
  "compare_cancel",
];
