// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EnvCompareApi` of the mock backend (wave 18, strand A): the placeholder until the strand replaces it.

import type { EnvCompareApi } from "./backend";
import { stubEnvCompare } from "./wave18-stubs";

export function mockEnvCompare(): EnvCompareApi {
  return stubEnvCompare();
}
