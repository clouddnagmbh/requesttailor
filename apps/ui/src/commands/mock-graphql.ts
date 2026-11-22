// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GraphQlApi` (wave 8, strand B). Interface placeholder; the strand replaces it.

import type { GraphQlApi } from "./backend";
import { stubGraphQl } from "./wave8-stubs";

export function mockGraphQl(): GraphQlApi {
  return stubGraphQl();
}
