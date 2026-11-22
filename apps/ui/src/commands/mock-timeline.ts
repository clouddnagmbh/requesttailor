// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `TimelineApi` of the mock backend (wave 12, strand D). Interface placeholder; the strand
// replaces it. `host` (./mock-wave12.ts) gives the mock's Git, requests, environments and
// sends; read it only inside the methods.

import type { TimelineApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import { stubTimeline } from "./wave12-stubs";

export function mockTimeline(host: MockWave12Host): TimelineApi {
  void host;
  return stubTimeline();
}
