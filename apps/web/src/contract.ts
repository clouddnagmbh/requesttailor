// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ContractApi` of the web app (wave 12, strand F). Interface placeholder; the strand replaces it.

import { stubContract, type ContractApi } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";

export function webContract(w: Wave12Host): ContractApi {
  void w;
  return stubContract();
}
