// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ViewsApi` of the mock backend (wave 18, strand G): the placeholder until the strand replaces it.

import type { ViewsApi } from "./backend";
import { stubViews } from "./wave18-stubs";

export function mockViews(): ViewsApi {
  return stubViews();
}
