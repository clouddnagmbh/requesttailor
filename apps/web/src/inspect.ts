// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InspectApi` of the web app (wave 18, strand B): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/inspect_js.rs`).

import { stubInspect, type InspectApi } from "@requesttailor/ui";

export function webInspect(): InspectApi {
  return stubInspect();
}
