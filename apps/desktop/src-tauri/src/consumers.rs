// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Consumer contracts for the desktop app (`Backend.consumers`, `rt_app::consumers`). Wave 19 interface:
//! registered, "not implemented yet"; strand F answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "Consumer contracts is not implemented yet (wave 19, strand F).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn consumers_list(
    _app: tauri::AppHandle,
) -> Result<Vec<rt_app::consumers::ConsumerRef>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn consumers_verify(
    _app: tauri::AppHandle,
    _request: rt_app::consumers::ConsumerVerifyRequest,
) -> Result<rt_app::consumers::ConsumerReport, CommandError> {
    not_yet()
}
