// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The RequestTailor desktop app: a Tauri 2 window showing the shared UI (`apps/ui`), whose
//! `Backend` calls the commands in `commands`, which forward to `rt_app` through [`Core`].
//!
//! Security: the webview gets no file system, shell or dialog access of its own. Its only
//! capability (`capabilities/default.json`) allows exactly the app's commands; the folder
//! picker runs on the Rust side. The content security policy is in `tauri.conf.json`.

mod commands;
pub mod core;
pub mod files;

pub use crate::core::{CollectionInfo, Core};

/// Starts the app. Returns when the last window is closed.
pub fn run() {
    // The app's keychain store (service "RequestTailor"); collection = canonical root path.
    let core = commands::AppCore::new(rt_app::secrets::keychain::KeychainStore::default())
        .expect("native transport");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(core)
        .invoke_handler(tauri::generate_handler![
            commands::open_collection,
            commands::tree,
            commands::get_request,
            commands::create_request,
            commands::update_request,
            commands::dirty,
            commands::save,
            commands::environments,
            commands::update_environment,
            commands::delete_environment,
            commands::preview,
            commands::send,
            commands::response_lines,
            commands::response_bytes,
            commands::filter_json,
            commands::release_response,
            commands::secret_has,
            commands::secret_get,
            commands::secret_set,
            commands::secret_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the RequestTailor desktop app");
}
