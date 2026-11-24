// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShareApi` of the mock backend (wave 17, strand C). Interface placeholder: every method
// rejects with kind `unsupported` until strand C fills this file in.

import type { ShareApi } from "./backend";
import { stubShare } from "./wave17-stubs";

export function mockShare(): ShareApi {
  return stubShare();
}
