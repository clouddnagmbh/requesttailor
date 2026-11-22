// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompareApi` of the mock backend (wave 12, strand C). Interface placeholder; the strand
// replaces it. `host` (./mock-wave12.ts) gives the mock's Git, requests, environments and
// sends; read it only inside the methods.

import type { CompareApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import { stubCompare } from "./wave12-stubs";

export function mockCompare(host: MockWave12Host): CompareApi {
  void host;
  return stubCompare();
}
