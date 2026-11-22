// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DiffApi` of the web app (wave 12, strand A). Interface placeholder; the strand replaces it.

import { stubDiffs, type DiffApi } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";

export function webDiffs(w: Wave12Host): DiffApi {
  void w;
  return stubDiffs();
}
