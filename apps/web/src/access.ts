// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AccessApi` of the web app (wave 19, strand D): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/access_js.rs`).

import { stubAccess, type AccessApi } from "@requesttailor/ui";

export function webAccess(): AccessApi {
  return stubAccess();
}
