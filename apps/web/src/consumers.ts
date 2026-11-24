// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ConsumersApi` of the web app (wave 19, strand F): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/consumers_js.rs`).

import { stubConsumers, type ConsumersApi } from "@requesttailor/ui";

export function webConsumers(): ConsumersApi {
  return stubConsumers();
}
