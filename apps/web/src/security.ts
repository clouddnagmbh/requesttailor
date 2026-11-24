// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SecurityApi` of the web app (wave 19, strand R): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/security_js.rs`).

import { stubSecurity, type SecurityApi } from "@requesttailor/ui";

export function webSecurity(): SecurityApi {
  return stubSecurity();
}
