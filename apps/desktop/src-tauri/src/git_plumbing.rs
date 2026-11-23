// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Commit-level building blocks of the active repository (`GitApi.commitInfo`, `pick`,
//! `writeTree`, `writeCommit`) and writing a resolved text conflict (`resolveText`);
//! `rt_app::git::plumbing`. Wave 15 interface: registered, "not implemented yet"; strand D
//! answers them from `rt-git`.

use rt_app::AppError;
use rt_app::git::plumbing::{
    GitCommitDetails, GitCommitWrite, GitPickRequest, GitPickResult, GitTreeWrite,
};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(
        AppError::Unsupported("Commit building blocks arrive with wave 15 (strand D).".into())
            .into(),
    )
}

#[tauri::command]
pub async fn git_commit_info(
    _app: tauri::AppHandle,
    rev: String,
) -> Result<GitCommitDetails, CommandError> {
    let _ = rev;
    not_yet()
}

#[tauri::command]
pub async fn git_pick(
    _app: tauri::AppHandle,
    request: GitPickRequest,
) -> Result<GitPickResult, CommandError> {
    let _ = request;
    not_yet()
}

#[tauri::command]
pub async fn git_write_tree(
    _app: tauri::AppHandle,
    write: GitTreeWrite,
) -> Result<String, CommandError> {
    let _ = write;
    not_yet()
}

#[tauri::command]
pub async fn git_write_commit(
    _app: tauri::AppHandle,
    write: GitCommitWrite,
) -> Result<String, CommandError> {
    let _ = write;
    not_yet()
}

#[tauri::command]
pub async fn git_resolve_text(
    _app: tauri::AppHandle,
    path: String,
    text: String,
) -> Result<(), CommandError> {
    let _ = (path, text);
    not_yet()
}
