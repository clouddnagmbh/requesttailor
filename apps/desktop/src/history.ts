// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `HistoryApi` of the desktop app (src-tauri/src/history.rs). Owner: wave 4, strand E.

import type { HistoryApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriHistory(call: Call): HistoryApi {
  return {
    list: (query, limit) => call("history_list", { query, limit }),
    clear: () => call("history_clear"),
  };
}
