// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DiffApi` of the desktop app (wave 12, strand A; src-tauri/src/diffs.rs): one Tauri command
// per method.

import type { DiffApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriDiffs(call: Call): DiffApi {
  return {
    working: () => call("diff_working"),
    commit: (rev) => call("diff_commit", { rev }),
    between: (from, to) => call("diff_between", { from, to }),
  };
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. */
export const PENDING: readonly Command[] = [];
