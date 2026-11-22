// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `StreamsApi` (wave 8, strand A). Interface placeholder; the strand replaces it.

import type { StreamsApi } from "./backend";
import { stubStreams } from "./wave8-stubs";

export function mockStreams(): StreamsApi {
  return stubStreams();
}
