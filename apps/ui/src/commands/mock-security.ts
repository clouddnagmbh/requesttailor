// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SecurityApi` of the mock backend (wave 19, strand R): the placeholder until the strand replaces it.

import type { SecurityApi } from "./backend";
import { stubSecurity } from "./wave19-stubs";

export function mockSecurity(): SecurityApi {
  return stubSecurity();
}
