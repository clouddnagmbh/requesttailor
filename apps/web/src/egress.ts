// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EgressApi` of the web app (wave 19, strand B): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/egress_js.rs`).

import { stubEgress, type EgressApi } from "@requesttailor/ui";

export function webEgress(): EgressApi {
  return stubEgress();
}
