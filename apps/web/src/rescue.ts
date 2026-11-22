// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RescueApi` of the web app (wave 12, strand G). Interface placeholder; the strand replaces it.

import { stubRescue, type RescueApi } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";

export function webRescue(w: Wave12Host): RescueApi {
  void w;
  return stubRescue();
}
