// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret rescue (`RescueApi`, `rt_app::rescue`): the history scan and the rewrite with `rt_git::rewrite`; the value goes into the keychain, never to the webview.
//!
//! Wave 12, strand G. Interface placeholder: the commands are registered (`build.rs`,
//! `capabilities/default.json`, `generate_handler!` in `lib.rs`, `COMMANDS` in
//! `../src/backend.ts`) and refuse until the strand implements them here; the strand adds
//! their parameters.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet() -> CommandError {
    AppError::Unsupported("Secret rescue arrives in wave 12 (strand G).".into()).into()
}

#[tauri::command]
pub async fn rescue_scan() -> Result<rt_app::rescue::HistoryScan, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn rescue_rewrite() -> Result<rt_app::rescue::RescueResult, CommandError> {
    Err(not_yet())
}
