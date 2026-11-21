// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The browser backend: rt-wasm (rt-app) + File System Access API + encrypted secret storage.
// Owner: wave 3, strand E. Until then it falls back to the mock so the shell stays usable.

import { MockBackend, type Backend } from "@requesttailor/ui";

export async function createWebBackend(): Promise<Backend> {
  console.warn("WebBackend not implemented yet (wave 3, strand E); using MockBackend.");
  return new MockBackend();
}
