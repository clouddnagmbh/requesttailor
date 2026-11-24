// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InspectApi` of the mock backend (wave 18, strand B): the placeholder until the strand replaces it.

import type { InspectApi } from "./backend";
import { stubInspect } from "./wave18-stubs";

export function mockInspect(): InspectApi {
  return stubInspect();
}
