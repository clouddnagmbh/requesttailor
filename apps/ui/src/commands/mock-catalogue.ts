// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CatalogueApi` of the mock backend (wave 18, strand I): the placeholder until the strand replaces it.

import type { CatalogueApi } from "./backend";
import { stubCatalogue } from "./wave18-stubs";

export function mockCatalogue(): CatalogueApi {
  return stubCatalogue();
}
