// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AccessApi` of the mock backend (wave 19, strand D): the placeholder until the strand replaces it.

import type { AccessApi } from "./backend";
import { stubAccess } from "./wave19-stubs";

export function mockAccess(): AccessApi {
  return stubAccess();
}
