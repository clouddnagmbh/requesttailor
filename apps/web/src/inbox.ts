// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InboxApi` of the web app (wave 18, strand E): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/inbox_js.rs`).

import { stubInbox, type InboxApi } from "@requesttailor/ui";

export function webInbox(): InboxApi {
  return stubInbox();
}
