// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// History and search in the mock backend, for their UI tests. Owner: wave 4, strand E.

import type { HistoryApi } from "./backend";
import type { RequestSpec, SearchHit, StoredResponse } from "./types";

export class MockHistory implements HistoryApi {
  /** Called by `MockBackend.send` after every send. */
  record(_collection: string, _spec: RequestSpec, _result: StoredResponse): void {}

  async list(): Promise<never[]> {
    return [];
  }

  async clear(): Promise<void> {}
}

/** Full-text search over the mock's requests. */
export function mockSearch(_requests: ReadonlyMap<string, RequestSpec>, _query: string, _limit: number): SearchHit[] {
  return [];
}
