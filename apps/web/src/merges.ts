// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `MergeApi` of the web app (wave 12, strand B). Interface placeholder; the strand replaces it.

import { stubMerges, type MergeApi } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";

export function webMerges(w: Wave12Host): MergeApi {
  void w;
  return stubMerges();
}
