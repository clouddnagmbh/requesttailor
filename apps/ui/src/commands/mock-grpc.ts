// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GrpcApi` (wave 8, strand B). Interface placeholder; the strand replaces it.

import type { GrpcApi } from "./backend";
import { stubGrpc } from "./wave8-stubs";

export function mockGrpc(): GrpcApi {
  return stubGrpc();
}
