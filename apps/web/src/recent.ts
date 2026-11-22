// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Recent collections of the web app (directory handles in IndexedDB, ./fs/recent.ts).
// `open(id, false)` succeeds only if the browser still grants access without asking (Chrome's
// "allow on every visit"). Owner: wave 4, strand C. Until then nothing is listed.

import type { CollectionInfo, RecentCollection } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

export interface WebRecent {
  /** Called by the backend with every collection it opened. */
  opened(info: CollectionInfo, handle: FileSystemDirectoryHandle): void;
  list(): Promise<RecentCollection[]>;
  open(id: string, interactive: boolean): Promise<CollectionInfo | null>;
}

export function webRecent(host: WebHost): WebRecent {
  void host;
  return {
    opened: () => {},
    list: async () => [],
    open: async () => null,
  };
}
