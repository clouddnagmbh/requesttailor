// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `TimelineApi` of the web app (wave 12, strand D), and the recorder every run of ./runner.ts
// reports to: results of a run of `HEAD`'s files are kept per commit in IndexedDB (never in
// the repository; `rt_app::timeline`). Interface placeholder; the strand replaces it.

import { stubTimeline, type RunEvent, type TimelineApi } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";
import type { Session } from "./web-backend";

export function webTimeline(w: Wave12Host): TimelineApi {
  void w;
  return stubTimeline();
}

/** Watches one run of the open collection; created before the run starts. Records nothing
 *  yet. */
export function runRecorder(session: Session): (event: RunEvent) => void {
  void session;
  return () => {};
}
