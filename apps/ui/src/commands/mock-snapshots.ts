// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SnapshotApi` of the mock backend (wave 17, strand B). Interface placeholder: every method
// rejects with kind `unsupported` until strand B fills this file in.

import type { SnapshotApi } from "./backend";
import { stubSnapshots } from "./wave17-stubs";

export function mockSnapshots(): SnapshotApi {
  return stubSnapshots();
}
