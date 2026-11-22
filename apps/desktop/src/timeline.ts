// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `TimelineApi` of the desktop app (wave 12, strand D; src-tauri/src/timeline.rs). Interface
// placeholder: the Tauri commands are registered and refuse; the strand calls them here.

import { stubTimeline, type TimelineApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriTimeline(call: Call): TimelineApi {
  void call;
  return stubTimeline();
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. The strand empties the list as it maps them. */
export const PENDING: readonly Command[] = [
  "timeline_history",
  "timeline_blame",
  "timeline_version",
  "timeline_results",
];
