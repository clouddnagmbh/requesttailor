// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The RequestTailor desktop app: a Tauri 2 window showing the shared UI (`apps/ui`), whose
//! `Backend` calls the commands in `commands`, which forward to `rt_app` through [`Core`].
//!
//! Security: the webview gets no file system, shell or dialog access of its own. Its only
//! capability (`capabilities/default.json`) allows exactly the app's commands and listening
//! to its events (the OAuth 2.0 `auth-prompt`; the system browser opens on the Rust side); the folder
//! picker runs on the Rust side. The content security policy is in `tauri.conf.json`.

/// Wave 8, strand C.
mod agents;
/// Wave 8, strand E.
mod ai;
mod auth;
mod commands;
mod cookies;
pub mod core;
pub mod files;
// Wave 4, one module per strand: git (A1), import (B), recent (C), history and search (E).
pub mod git;
/// Wave 8, strand B.
mod grpc;
pub mod history;
mod import;
/// Wave 8, strand D.
mod mcp_client;
mod plugins;
pub mod recent;
// Wave 7: scripts (strand A), session variables (C), runner (D).
mod runner;
mod scripts;
mod search;
mod services;
mod session_vars;
/// Wave 8, strand A.
mod streams;

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
            // OAuth 2.0 prompts reach the UI as the `auth-prompt` event (wave 5, strand C).
            core.services.auth.attach(app.handle().clone());
            // Bundled plugins (wave 6): only located here; installed on the first
            // `plugin_welcome`, after the first frame.
            let resources = app.path().resource_dir().ok();
            core.services.plugins.bundle_from(
                plugins::PluginParts::bundled_dirs(resources.as_deref()),
                plugins::BUNDLED,
            );
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_collection,
            commands::tree,
            commands::get_request,
            commands::create_request,
            commands::create_folder,
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
            git::git_scan,
            plugins::import_analyze_plugin,
            plugins::plugin_list,
            plugins::plugin_inspect,
            plugins::plugin_install,
            plugins::plugin_remove,
            plugins::plugin_set_enabled,
            plugins::plugin_render_tab,
            plugins::plugin_action,
            plugins::plugin_welcome,
            plugins::plugin_choose_welcome,
            auth::auth_tokens,
            auth::auth_fetch_token,
            auth::auth_clear_tokens,
            auth::auth_cancel_prompt,
            cookies::cookie_list,
            cookies::cookie_delete,
            cookies::cookie_clear,
            scripts::scripts_trust,
            scripts::scripts_set_trusted,
            session_vars::scripts_session_vars,
            session_vars::scripts_set_session_var,
            session_vars::scripts_clear_session_vars,
            runner::runner_plan,
            runner::runner_parse_data,
            runner::runner_run,
            runner::runner_cancel,
            agents::agents_policy,
            agents::agents_set_policy,
            agents::agents_pending,
            agents::agents_decide,
            agents::agents_audit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the RequestTailor desktop app");
}
