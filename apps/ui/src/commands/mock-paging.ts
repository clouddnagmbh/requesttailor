// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `PagingApi` of the mock backend (wave 18, strand C): the placeholder until the strand replaces it.

import type { PagingApi } from "./backend";
import { stubPaging } from "./wave18-stubs";

export function mockPaging(): PagingApi {
  return stubPaging();
}
