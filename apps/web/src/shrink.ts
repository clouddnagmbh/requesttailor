// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShrinkApi` of the web app (wave 19, strand E): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/shrink_js.rs`).

import { stubShrink, type ShrinkApi } from "@requesttailor/ui";

export function webShrink(): ShrinkApi {
  return stubShrink();
}
