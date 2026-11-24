// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShrinkApi` of the mock backend (wave 19, strand E): the placeholder until the strand replaces it.

import type { ShrinkApi } from "./backend";
import { stubShrink } from "./wave19-stubs";

export function mockShrink(): ShrinkApi {
  return stubShrink();
}
