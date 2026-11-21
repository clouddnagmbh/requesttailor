// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Generates one `allow-<command>` permission per app command, so the capability in
//! `capabilities/default.json` lists exactly what the webview may call. Keep this list in
//! sync with `generate_handler!` in `src/lib.rs` and `COMMANDS` in `../src/backend.ts`
//! (`npm test -w apps/desktop` checks the TypeScript side against the capability).

const COMMANDS: &[&str] = &[
    "open_collection",
    "tree",
    "get_request",
    "create_request",
    "update_request",
    "dirty",
    "save",
    "environments",
    "update_environment",
    "delete_environment",
    "preview",
    "send",
    "response_lines",
    "response_bytes",
    "filter_json",
    "release_response",
    "secret_has",
    "secret_get",
    "secret_set",
    "secret_delete",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri-build");
}
