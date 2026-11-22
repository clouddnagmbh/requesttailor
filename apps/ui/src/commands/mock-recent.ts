// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Recent collections in the mock backend, for the start flow's tests. Owner: wave 4, strand C.

import type { CollectionInfo, RecentCollection } from "./backend";

export class MockRecent {
  /** Called by `MockBackend.openCollection` with what it opened. */
  opened(_info: CollectionInfo): void {}

  list(): RecentCollection[] {
    return [];
  }
}
