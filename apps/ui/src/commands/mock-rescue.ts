// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RescueApi` of the mock backend (wave 12, strand G). Interface placeholder; the strand
// replaces it. `host` (./mock-wave12.ts) gives the mock's Git, requests, environments and
// sends; read it only inside the methods.

import type { RescueApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import { stubRescue } from "./wave12-stubs";

export function mockRescue(host: MockWave12Host): RescueApi {
  void host;
  return stubRescue();
}
