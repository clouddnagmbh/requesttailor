// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Saved views for the desktop app (`Backend.views`, `rt_app::views`). The file
//! `<request>.views.json` is staged in the active collection like any companion file (unsaved
//! until `save`); a view is evaluated here on the kept response, only the shaped result goes to
//! the webview.

use rt_app::views::{self, ResponseView, ViewLanguage, ViewResult};
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn views_list(
    core: State<'_, AppCore>,
    path: String,
) -> Result<Vec<ResponseView>, CommandError> {
    Ok(core.with_workspace(|ws, _| views::list(ws, &path))?)
}

#[tauri::command]
pub async fn views_save(
    core: State<'_, AppCore>,
    path: String,
    view: ResponseView,
) -> Result<Vec<ResponseView>, CommandError> {
    Ok(core.with_workspace(|ws, _| views::save(ws, &path, view))?)
}

#[tauri::command]
pub async fn views_remove(
    core: State<'_, AppCore>,
    path: String,
    name: String,
) -> Result<Vec<ResponseView>, CommandError> {
    Ok(core.with_workspace(|ws, _| views::remove(ws, &path, &name))?)
}

#[tauri::command]
pub async fn views_apply(
    core: State<'_, AppCore>,
    path: String,
    response_id: u64,
    view: ResponseView,
) -> Result<ViewResult, CommandError> {
    let _ = path;
    Ok(core.with_responses(|_, responses| views::apply(responses, response_id, &view))?)
}

#[tauri::command]
pub async fn views_languages() -> Result<Vec<ViewLanguage>, CommandError> {
    Ok(views::languages())
}
