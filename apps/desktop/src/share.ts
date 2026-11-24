// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand C: sharing without a server and copying as code, one Tauri command per method
// (`src-tauri/src/share.rs`, placeholders until strand C).

import type { ShareApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriShare(call: Call): ShareApi {
  return {
    link: (path, options) => call("share_link", { path, options: options ?? {} }),
    open: (input) => call("share_open", { input }),
    copyAs: (path, kind, options) => call("share_copy_as", { path, kind, options: options ?? {} }),
    qr: (link) => call("share_qr", { link }),
  };
}
