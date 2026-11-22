// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompareApi` of the web app (wave 12, strand C). Interface placeholder; the strand replaces it.

import { stubCompare, type CompareApi } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";

export function webCompare(w: Wave12Host): CompareApi {
  void w;
  return stubCompare();
}
