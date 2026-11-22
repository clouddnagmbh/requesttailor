// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `HistoryApi` of the web app, persisted in IndexedDB. `record` is called by the backend after
// every send. Owner: wave 4, strand E. Until then nothing is recorded.

import type { HistoryApi, HistoryEntry, RequestSpec, StoredResponse } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

export class WebHistory implements HistoryApi {
  constructor(private readonly host: WebHost) {}

  /** Never throws into the send. */
  record(collection: string, spec: RequestSpec, result: StoredResponse): void {
    void [this.host, collection, spec, result];
  }

  async list(query: string, limit: number): Promise<HistoryEntry[]> {
    void [query, limit];
    return [];
  }

  async clear(): Promise<void> {}
}
