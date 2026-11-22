// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `MergeApi` of the mock backend (wave 12, strand B). Interface placeholder; the strand
// replaces it. `host` (./mock-wave12.ts) gives the mock's Git, requests, environments and
// sends; read it only inside the methods.

import type { MergeApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import { stubMerges } from "./wave12-stubs";

export function mockMerges(host: MockWave12Host): MergeApi {
  void host;
  return stubMerges();
}
