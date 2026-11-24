// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Saved views for the desktop app (`Backend.views`, `rt_app::views`). Wave 18 interface:
//! registered, "not implemented yet"; strand G answers them.

use rt_app::AppError;
use rt_app::views::{ResponseView, ViewLanguage, ViewResult};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(
        AppError::Unsupported("Saved views is not implemented yet (wave 18, strand G).".into())
            .into(),
    )
}

#[tauri::command]
pub async fn views_list(
    _app: tauri::AppHandle,
    path: String,
) -> Result<Vec<ResponseView>, CommandError> {
    let _ = (path,);
    not_yet()
}

#[tauri::command]
pub async fn views_save(
    _app: tauri::AppHandle,
    path: String,
    view: ResponseView,
) -> Result<Vec<ResponseView>, CommandError> {
    let _ = (path, view);
    not_yet()
}

#[tauri::command]
pub async fn views_remove(
    _app: tauri::AppHandle,
    path: String,
    name: String,
) -> Result<Vec<ResponseView>, CommandError> {
    let _ = (path, name);
    not_yet()
}

#[tauri::command]
pub async fn views_apply(
    _app: tauri::AppHandle,
    path: String,
    response_id: u64,
    view: ResponseView,
) -> Result<ViewResult, CommandError> {
    let _ = (path, response_id, view);
    not_yet()
}

#[tauri::command]
pub async fn views_languages(_app: tauri::AppHandle) -> Result<Vec<ViewLanguage>, CommandError> {
    not_yet()
}
