// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `PagingApi` of the web app (wave 18, strand C): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/paging_js.rs`).

import { stubPaging, type PagingApi } from "@requesttailor/ui";

export function webPaging(): PagingApi {
  return stubPaging();
}
