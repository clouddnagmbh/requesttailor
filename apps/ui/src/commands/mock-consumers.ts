// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ConsumersApi` of the mock backend (wave 19, strand F): the placeholder until the strand replaces it.

import type { ConsumersApi } from "./backend";
import { stubConsumers } from "./wave19-stubs";

export function mockConsumers(): ConsumersApi {
  return stubConsumers();
}
