// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Secret values in the browser: encrypted (AES-GCM via WebCrypto) in IndexedDB, with a
// non-extractable key that never leaves this browser profile. Never in collection files,
// never in localStorage in clear text. Owner: wave 3, strand D.

import type { SecretsApi } from "@requesttailor/ui";

/** Secret store for one collection (`collectionId` = `CollectionInfo.id`). */
export async function openBrowserSecretStore(collectionId: string): Promise<SecretsApi> {
  void collectionId;
  throw new Error("openBrowserSecretStore: wave 3, strand D");
}
