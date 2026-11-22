// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AiApi` (wave 8, strand E). Interface placeholder; the strand replaces it.

import type { AiApi } from "./backend";
import { stubAi } from "./wave8-stubs";

export function mockAi(): AiApi {
  return stubAi();
}
