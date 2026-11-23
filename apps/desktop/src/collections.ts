// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CollectionSetApi` of the desktop app (wave 14, strand A; src-tauri/src/collections.rs): the
// collection set lives in the Rust core, which walks the folders and remembers the roots in the
// data folder. Folder pickers run on the Rust side.

import type { CollectionSetApi } from "@requesttailor/ui";

import type { Call } from "./backend";

/** With `reveal` ("Show in Finder"), which only the desktop has. */
export function tauriCollections(call: Call): Required<CollectionSetApi> {
  return {
    current: () => call("collections_current"),
    add: () => call("collections_add"),
    removeRoot: (root) => call("collections_remove_root", { root }),
    activate: (id) => call("collections_activate", { id }),
    tree: (id) => call("collections_tree", { id }),
    dirty: (id) => call("collections_dirty", { id }),
    saveAll: () => call("collections_save_all"),
    search: (query, limit) => call("collections_search", { query, limit }),
    create: (spec) => call("collections_create", { spec }),
    reveal: (id) => call("collections_reveal", { id }),
  };
}
