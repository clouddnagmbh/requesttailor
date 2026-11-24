// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EgressApi` of the mock backend (wave 19, strand B): the placeholder until the strand replaces it.

import type { EgressApi } from "./backend";
import { stubEgress } from "./wave19-stubs";

export function mockEgress(): EgressApi {
  return stubEgress();
}
