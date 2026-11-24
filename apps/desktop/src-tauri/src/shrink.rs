// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The minimal failing request for the desktop app (`Backend.shrink`, `rt_app::shrink`). Wave 19 interface:
//! registered, "not implemented yet"; strand E answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "The minimal failing request is not implemented yet (wave 19, strand E).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn shrink_run(
    _app: tauri::AppHandle,
    _request: rt_app::shrink::ShrinkRequest,
) -> Result<rt_app::shrink::ShrinkResult, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn shrink_cancel(_app: tauri::AppHandle) -> Result<(), CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn shrink_predicate(
    _app: tauri::AppHandle,
    _path: String,
    _response_id: Option<u32>,
) -> Result<rt_app::shrink::Predicate, CommandError> {
    not_yet()
}
