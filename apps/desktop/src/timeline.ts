// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `TimelineApi` of the desktop app (wave 12, strand D; src-tauri/src/timeline.rs): one Tauri
// command per method. Test results per commit are recorded on the Rust side by the runner's
// `RunRecorder`; nothing here writes them.

import type { TimelineApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriTimeline(call: Call): TimelineApi {
  return {
    history: (path, limit) => call("timeline_history", { path, limit }),
    blame: (path) => call("timeline_blame", { path }),
    version: (rev, path) => call("timeline_version", { rev, path }),
    results: (rev) => call("timeline_results", { rev }),
  };
}
