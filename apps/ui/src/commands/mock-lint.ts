// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `LintApi` of the mock backend (wave 18, strand F): the placeholder until the strand replaces it.

import type { LintApi } from "./backend";
import { stubLint } from "./wave18-stubs";

export function mockLint(): LintApi {
  return stubLint();
}
