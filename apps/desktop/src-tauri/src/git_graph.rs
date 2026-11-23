// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The commit graph of the active collection's repository (`GitApi.graph`, `refs`, `changes`,
//! `fileDiff`; `rt_app::git::graph`). Wave 14, strand F: natively `rt-git/src/graph.rs`.
//! Placeholders until then.

use rt_app::AppError;
use rt_app::git::GitDiff;
use rt_app::git::graph::{GitChangedFile, GitGraphPage, GitGraphQuery, GitRef};

use crate::commands::CommandError;

fn later<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported("the commit graph arrives in wave 14 (strand F)".into()).into())
}

#[tauri::command]
pub async fn git_graph(query: GitGraphQuery) -> Result<GitGraphPage, CommandError> {
    let _ = query;
    later()
}

#[tauri::command]
pub async fn git_refs() -> Result<Vec<GitRef>, CommandError> {
    later()
}

#[tauri::command]
pub async fn git_changes(
    from: Option<String>,
    to: String,
) -> Result<Vec<GitChangedFile>, CommandError> {
    let _ = (from, to);
    later()
}

#[tauri::command]
pub async fn git_file_diff(
    from: Option<String>,
    to: Option<String>,
    path: String,
) -> Result<GitDiff, CommandError> {
    let _ = (from, to, path);
    later()
}
