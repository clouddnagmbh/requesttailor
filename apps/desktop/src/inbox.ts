// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand E: the webhook inbox, one Tauri command per method (`src-tauri/src/inbox.rs`).

import type { InboxApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriInbox(call: Call): InboxApi {
  return {
    settings: () => call("inbox_settings"),
    setSettings: (settings) => call("inbox_set_settings", { settings }),
    start: () => call("inbox_start"),
    stop: () => call("inbox_stop"),
    status: () => call("inbox_status"),
    events: (since) => call("inbox_events", { since }),
    get: (id) => call("inbox_get", { id }),
    clear: () => call("inbox_clear"),
    makeRequest: (id) => call("inbox_make_request", { id }),
  };
}
