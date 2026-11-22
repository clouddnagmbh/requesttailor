// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DiffApi` of the mock backend (wave 12, strand A). Interface placeholder; the strand
// replaces it. `host` (./mock-wave12.ts) gives the mock's Git, requests, environments and
// sends; read it only inside the methods.

import type { DiffApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import { stubDiffs } from "./wave12-stubs";

export function mockDiffs(host: MockWave12Host): DiffApi {
  void host;
  return stubDiffs();
}
