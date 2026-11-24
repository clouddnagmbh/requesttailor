// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! API notebooks for the desktop app (`Backend.notebook`, `rt_app::notebook`). Wave 19 interface:
//! registered, "not implemented yet"; strand W answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(
        AppError::Unsupported("API notebooks is not implemented yet (wave 19, strand W).".into())
            .into(),
    )
}

#[tauri::command]
pub async fn notebook_list(
    _app: tauri::AppHandle,
) -> Result<Vec<rt_app::notebook::NotebookEntry>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn notebook_read(
    _app: tauri::AppHandle,
    _path: String,
) -> Result<rt_app::notebook::Notebook, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn notebook_stage(
    _app: tauri::AppHandle,
    _path: String,
    _notebook: rt_app::notebook::Notebook,
) -> Result<(), CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn notebook_run(
    _app: tauri::AppHandle,
    _run: rt_app::notebook::NotebookRun,
) -> Result<rt_app::notebook::NotebookOutcome, CommandError> {
    not_yet()
}
