// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand B: snapshot tests, one Tauri command per method (`src-tauri/src/snapshots.rs`,
// placeholders until strand B).

import type { SnapshotApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriSnapshots(call: Call): SnapshotApi {
  return {
    read: (path) => call("snapshots_read", { path }),
    freeze: (path, responseId, options) =>
      call("snapshots_freeze", { path, responseId, options: options ?? {} }),
    setIgnore: (path, ignore) => call("snapshots_set_ignore", { path, ignore }),
    check: (path, responseId) => call("snapshots_check", { path, responseId }),
    list: () => call("snapshots_list"),
  };
}
