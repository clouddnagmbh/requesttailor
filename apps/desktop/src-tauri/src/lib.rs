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
/// Wave 12, strand C: runs at a revision, comparison, bisect.
mod compare;
/// Wave 12, strand F: OpenAPI contract.
mod contract;
mod cookies;
pub mod core;
/// Wave 12, strand A: semantic diff.
mod diffs;
pub mod files;
// Wave 4, one module per strand: git (A1), import (B), recent (C), history and search (E).
pub mod git;
/// Wave 8, strand B.
mod grpc;
pub mod history;
mod import;
/// Wave 8, strand D.
mod mcp_client;
/// Wave 12, strand B: semantic merge.
mod merges;
mod plugins;
pub mod recent;
/// Wave 12, strand G: secret rescue.
pub mod rescue;
/// Wave 12, strand E: pull-request review.
mod review;
// Wave 7: scripts (strand A), session variables (C), runner (D).
mod runner;
mod scripts;
mod search;
mod services;
mod session_vars;
/// Wave 8, strand A.
mod streams;
/// Wave 12, strand D: request history and test results per commit.
mod timeline;

/// The app's core as managed Tauri state (wave 10). A distribution's Tauri plugin may use it,
/// e.g. to open a collection it placed on disk: `app.state::<AppCore>().open(&root)`, then
/// its UI extension shows it with `ctx.app.adopt(info)`.
pub use crate::commands::AppCore;
pub use crate::core::{CollectionInfo, Core};

/// The configured app (plugins, state, commands) without its context. `main.rs` runs it with
/// the context of this crate's `tauri.conf.json`; a distribution that builds its own binary
/// from this crate: it adds its Tauri plugins with `.plugin(…)` and runs
/// with the context of its own `tauri.conf.json` (wave 9). Its plugins' commands need their own
/// capability file; the app's commands keep `capabilities/default.json`.
///
/// Wave 10: a distribution that checks every send manages its guard before `run`:
/// `builder().manage(rt_core::guard::GuardHandle::new(guard))`; the app's sends (Send, runner,
/// streams, gRPC) then ask it (`rt_core::guard`).
pub fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager as _;
            // History and recent collections live in the app's data folder: the same folder
            // `rt` uses (`$RT_DATA_DIR`, else the folder of this app's identifier), also when a
            // distribution runs under another bundle identifier.
            let data_dir =
                rt_plugin::host::dir::data_dir().or_else(|| app.path().app_data_dir().ok());
            // The app's keychain store (service "RequestTailor"); collection = canonical root.
            // Wave 10: a distribution's send guard, managed on the builder before `run`.
            let guard = app
                .try_state::<rt_core::guard::GuardHandle>()
                .map(|g| g.inner().clone())
                .unwrap_or_default();
            let core = commands::AppCore::with_data_dir(
                rt_app::secrets::keychain::KeychainStore::default(),
                data_dir,
            )?
            .with_guard(guard);
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
            ai::ai_config,
            ai::ai_set_config,
            ai::ai_has_key,
            ai::ai_set_key,
            ai::ai_delete_key,
            ai::ai_ask,
            agents::agents_policy,
            agents::agents_set_policy,
            agents::agents_pending,
            agents::agents_decide,
            agents::agents_audit,
            // Wave 8, strand B.
            grpc::grpc_services,
            grpc::grpc_template,
            grpc::graphql_schema,
            // Wave 8, strand A.
            streams::stream_open,
            streams::stream_send,
            streams::stream_finish,
            streams::stream_close,
            streams::stream_log,
            streams::stream_release,
            mcp_client::mcp_connect,
            mcp_client::mcp_tools,
            mcp_client::mcp_call,
            mcp_client::mcp_disconnect,
            mcp_client::mcp_tests,
            mcp_client::mcp_get_test,
            mcp_client::mcp_save_test,
            mcp_client::mcp_run_test,
            // Wave 12 (interface): revisions.
            git::git_rev_parse,
            git::git_tree_at,
            git::git_show,
            git::git_file_log,
            git::git_merge_base,
            git::git_commits,
            // Wave 12, strand A.
            diffs::diff_working,
            diffs::diff_commit,
            diffs::diff_between,
            // Wave 12, strand B.
            merges::merge_conflicts,
            merges::merge_resolve,
            // Wave 12, strand C.
            compare::compare_run,
            compare::compare_revisions,
            compare::compare_bisect,
            compare::compare_cancel,
            // Wave 12, strand D.
            timeline::timeline_history,
            timeline::timeline_blame,
            timeline::timeline_version,
            timeline::timeline_results,
            // Wave 12, strand E.
            review::review_hosts,
            review::review_set_hosts,
            review::review_repo,
            review::review_has_token,
            review::review_set_token,
            review::review_delete_token,
            review::review_start_login,
            review::review_finish_login,
            review::review_list,
            review::review_open,
            review::review_comments,
            review::review_comment,
            review::review_submit,
            review::review_ci,
            // Wave 12, strand F.
            contract::contract_spec,
            contract::contract_check,
            contract::contract_changes,
            contract::contract_changelog,
            // Wave 12, strand G.
            rescue::rescue_scan,
            rescue::rescue_rewrite,
        ])
}
