// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `McpClientApi` (wave 8, strand D). Interface placeholder; the strand replaces it.

import type { McpClientApi } from "./backend";
import { stubMcp } from "./wave8-stubs";

export function mockMcp(): McpClientApi {
  return stubMcp();
}
