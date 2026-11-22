// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GrpcApi` (wave 8, strand B). Interface placeholder; the strand replaces it.

import { stubGrpc, type GrpcApi } from "@requesttailor/ui";

export function webGrpc(): GrpcApi {
  return stubGrpc();
}
