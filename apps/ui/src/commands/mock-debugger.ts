// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DebuggerApi` of the mock backend (wave 19, strand Y): the placeholder until the strand replaces it.

import type { DebuggerApi } from "./backend";
import { stubDebugger } from "./wave19-stubs";

export function mockDebugger(): DebuggerApi {
  return stubDebugger();
}
