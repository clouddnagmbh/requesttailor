// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14 interface: a placeholder `CollectionSetApi` over the single open collection, until
// strand A replaces it in each shell's own file (desktop `src/collections.ts`, web
// `src/collections.ts`, mock `mock-collections.ts`). It knows one collection: reads go to the
// existing single-collection commands, set changes reject with `unsupported`.

import type { Backend, CollectionSetApi } from "./backend";
import { BackendError } from "./backend";

const notYet = () =>
  Promise.reject(new BackendError("unsupported", "Several collections in the sidebar arrive in wave 14 (strand A)."));

export function stubCollections(single: Pick<Backend, "tree" | "dirty" | "save" | "search">): CollectionSetApi {
  return {
    current: async () => ({ roots: [], collections: [], active: null }),
    add: notYet,
    removeRoot: notYet,
    activate: notYet,
    tree: () => single.tree(),
    dirty: () => single.dirty(),
    saveAll: async () => {
      const changes = await single.save();
      return changes.length > 0 ? [{ collection: "", changes }] : [];
    },
    search: async (query, limit) => (await single.search(query, limit)).map((hit) => ({ ...hit, collection: "" })),
    create: notYet,
  };
}
