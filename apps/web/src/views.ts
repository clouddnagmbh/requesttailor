// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ViewsApi` of the web app (wave 18, strand G): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/views_js.rs`).

import { stubViews, type ViewsApi } from "@requesttailor/ui";

export function webViews(): ViewsApi {
  return stubViews();
}
