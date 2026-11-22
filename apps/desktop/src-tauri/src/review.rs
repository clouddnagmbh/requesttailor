// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Pull-request review (`ReviewApi`, `rt_app::review`): tokens in the keychain (`rt_app::review::token_slot`), providers over the app's transport, PR heads fetched with `rt_git::review`.
//!
//! Wave 12, strand E. Interface placeholder: the commands are registered (`build.rs`,
//! `capabilities/default.json`, `generate_handler!` in `lib.rs`, `COMMANDS` in
//! `../src/backend.ts`) and refuse until the strand implements them here; the strand adds
//! their parameters.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet() -> CommandError {
    AppError::Unsupported("Pull-request review arrives in wave 12 (strand E).".into()).into()
}

#[tauri::command]
pub async fn review_hosts() -> Result<Vec<rt_app::review::ReviewHost>, CommandError> {
    Ok(Vec::new())
}

#[tauri::command]
pub async fn review_set_hosts() -> Result<(), CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_repo() -> Result<Option<rt_app::review::ReviewRepo>, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_has_token() -> Result<bool, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_set_token() -> Result<(), CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_delete_token() -> Result<(), CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_start_login() -> Result<rt_app::auth::DeviceCodePrompt, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_finish_login() -> Result<(), CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_list() -> Result<Vec<rt_app::review::PullRequest>, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_open() -> Result<rt_app::review::ReviewSession, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_comments() -> Result<Vec<rt_app::review::ReviewComment>, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_comment() -> Result<rt_app::review::ReviewComment, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_submit() -> Result<(), CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn review_ci() -> Result<rt_app::review::CiStatus, CommandError> {
    Err(not_yet())
}
