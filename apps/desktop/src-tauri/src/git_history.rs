// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Blame, history search and reflog of the active repository (`GitApi.blame`, `searchHistory`,
//! `reflog`; `rt_app::git::history`). Wave 15 interface: registered, "not implemented yet";
//! strand C answers them from `rt-git`.

use rt_app::AppError;
use rt_app::git::history::{GitBlame, GitHistoryPage, GitHistoryQuery, GitReflogEntry};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "Blame, history search and the reflog arrive with wave 15 (strand C).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn git_blame(
    _app: tauri::AppHandle,
    rev: String,
    path: String,
) -> Result<GitBlame, CommandError> {
    let _ = (rev, path);
    not_yet()
}

#[tauri::command]
pub async fn git_search_history(
    _app: tauri::AppHandle,
    query: GitHistoryQuery,
) -> Result<GitHistoryPage, CommandError> {
    let _ = query;
    not_yet()
}

#[tauri::command]
pub async fn git_reflog(
    _app: tauri::AppHandle,
    name: String,
    limit: u32,
) -> Result<Vec<GitReflogEntry>, CommandError> {
    let _ = (name, limit);
    not_yet()
}
