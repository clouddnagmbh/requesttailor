// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AgentsApi` (wave 8, strand C). Interface placeholder; the strand replaces it.

import type { AgentsApi } from "./backend";
import { stubAgents } from "./wave8-stubs";

export function mockAgents(): AgentsApi {
  return stubAgents();
}
