// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ReviewApi` of the web app (wave 12, strand E). Interface placeholder; the strand replaces it.

import { stubReview, type ReviewApi } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";

export function webReview(w: Wave12Host): ReviewApi {
  void w;
  return stubReview();
}
