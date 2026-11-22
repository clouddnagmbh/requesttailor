// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RescueApi` of the desktop app (wave 12, strand G; src-tauri/src/rescue.rs). Interface
// placeholder: the Tauri commands are registered and refuse; the strand calls them here.

import { stubRescue, type RescueApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriRescue(call: Call): RescueApi {
  void call;
  return stubRescue();
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. The strand empties the list as it maps them. */
export const PENDING: readonly Command[] = [
  "rescue_scan",
  "rescue_rewrite",
];
