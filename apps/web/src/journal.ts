// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `JournalApi` of the web app (wave 19, strand C): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/journal_js.rs`).

import { stubJournal, type JournalApi } from "@requesttailor/ui";

export function webJournal(): JournalApi {
  return stubJournal();
}
