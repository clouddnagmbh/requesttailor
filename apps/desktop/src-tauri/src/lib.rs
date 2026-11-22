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
// Wave 4, one module per strand: git (A1), import (B), recent (C), history and search (E).
mod git;
pub mod history;
mod import;
pub mod recent;
mod search;

pub use crate::core::{CollectionInfo, Core};

/// Starts the app. Returns when the last window is closed.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager as _;
            // History and recent collections live in the app's data folder.
            let data_dir = app.path().app_data_dir().ok();
            // The app's keychain store (service "RequestTailor"); collection = canonical root.
            let core = commands::AppCore::with_data_dir(
                rt_app::secrets::keychain::KeychainStore::default(),
                data_dir,
            )?;
            app.manage(core);
            Ok(())
        })
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
            git::git_status,
            git::git_diff,
            git::git_commit,
            git::git_log,
            git::git_branches,
            git::git_checkout,
            git::git_create_branch,
            git::git_push,
            git::git_pull,
            git::git_conflict,
            git::git_resolve,
            git::git_init,
            git::git_clone,
            git::git_identity,
            git::git_set_identity,
            git::git_has_credentials,
            git::git_set_credentials,
            git::git_delete_credentials,
            import::import_analyze,
            import::import_write,
            import::import_discard,
            history::history_list,
            history::history_clear,
            search::search,
            recent::recent_collections,
            recent::open_recent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the RequestTailor desktop app");
}
