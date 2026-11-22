// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ContractApi` of the mock backend (wave 12, strand F). Interface placeholder; the strand
// replaces it. `host` (./mock-wave12.ts) gives the mock's Git, requests, environments and
// sends; read it only inside the methods.

import type { ContractApi } from "./backend";
import type { MockWave12Host } from "./mock-wave12";
import { stubContract } from "./wave12-stubs";

export function mockContract(host: MockWave12Host): ContractApi {
  void host;
  return stubContract();
}
