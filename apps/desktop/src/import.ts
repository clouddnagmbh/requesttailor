// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ImportApi` of the desktop app (src-tauri/src/import.rs). Owner: wave 4, strand B.

import type { ImportApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriImports(call: Call): ImportApi {
  return {
    // `Vec<u8>` on the Rust side: a plain array of numbers over JSON IPC.
    analyze: (fileName, data, format) => call("import_analyze", { fileName, data: Array.from(data), format }),
    write: (id) => call("import_write", { id }),
    discard: (id) => call("import_discard", { id }),
  };
}
