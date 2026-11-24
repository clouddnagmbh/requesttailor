// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CatalogueApi` of the web app (wave 18, strand I): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/catalogue_js.rs`).

import { stubCatalogue, type CatalogueApi } from "@requesttailor/ui";

export function webCatalogue(): CatalogueApi {
  return stubCatalogue();
}
