// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand C: the test-data journal, one Tauri command per method (`src-tauri/src/journal.rs`,
// placeholders until strand C).

import type { JournalApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriJournal(call: Call): JournalApi {
  return {
    settings: () => call("journal_settings"),
    setSettings: (settings) => call("journal_set_settings", { settings }),
    entries: (query) => call("journal_entries", { query }),
    clear: () => call("journal_clear"),
    planManual: (revert) => call("journal_plan_manual", { revert }),
    planCompensations: (items) => call("journal_plan_compensations", { items }),
    execute: (plan, options) => call("journal_execute", { plan, options }),
    pluginRules: () => call("journal_plugin_rules"),
    preRead: (request, environment) => call("journal_pre_read", { request, environment }),
    states: (entryIds) => call("journal_states", { entryIds }),
  };
}
