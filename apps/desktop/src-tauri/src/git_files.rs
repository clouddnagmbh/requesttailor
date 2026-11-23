// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The working copy, file tree and file contents of the active repository (`GitApi.workingCopy`,
//! `fileTree`, `blob`; `rt_app::git::files`). Wave 15 interface: registered, "not implemented
//! yet"; strand B answers them from `rt-git` (budget `git_status_large`).

use rt_app::AppError;
use rt_app::git::files::{GitBlob, GitTreeEntry, GitWorkingCopy};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "The working copy of any repository arrives with wave 15 (strand B).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn git_working_copy(_app: tauri::AppHandle) -> Result<GitWorkingCopy, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn git_file_tree(
    _app: tauri::AppHandle,
    rev: Option<String>,
    dir: String,
) -> Result<Vec<GitTreeEntry>, CommandError> {
    let _ = (rev, dir);
    not_yet()
}

#[tauri::command]
pub async fn git_blob(
    _app: tauri::AppHandle,
    rev: Option<String>,
    path: String,
) -> Result<GitBlob, CommandError> {
    let _ = (rev, path);
    not_yet()
}
