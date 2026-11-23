// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Repositories in the collection set (`Backend.repositories`, `rt_app::git::repos`). Wave 15
//! interface: the commands are registered and answer "not implemented yet"; strand A opens any
//! folder in a Git repository as a repository root, keeps the active repository in
//! [`crate::core::Core`] (`active_repository`) and clones or creates repositories.

use rt_app::AppError;
use rt_app::collections::CollectionSet;
use rt_app::git::repos::{GitCloneRequest, GitRepoSummary, RepositoryEntry};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "Repositories without a collection arrive with wave 15 (strand A).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn repositories_open(
    _app: tauri::AppHandle,
) -> Result<Option<CollectionSet>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn repositories_activate(
    _app: tauri::AppHandle,
    id: String,
) -> Result<RepositoryEntry, CommandError> {
    let _ = id;
    not_yet()
}

#[tauri::command]
pub async fn repositories_summary(
    _app: tauri::AppHandle,
    id: String,
) -> Result<GitRepoSummary, CommandError> {
    let _ = id;
    not_yet()
}

#[tauri::command]
pub async fn repositories_clone(
    _app: tauri::AppHandle,
    request: GitCloneRequest,
) -> Result<Option<RepositoryEntry>, CommandError> {
    let _ = request;
    not_yet()
}

#[tauri::command]
pub async fn repositories_init(
    _app: tauri::AppHandle,
) -> Result<Option<RepositoryEntry>, CommandError> {
    not_yet()
}
