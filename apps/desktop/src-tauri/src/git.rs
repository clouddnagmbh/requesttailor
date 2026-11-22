// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Git commands of the desktop app (`GitApi` in `apps/ui/src/commands/backend.ts`), backed by
//! the `rt-git` crate (gitoxide) on [`crate::core::Core::root`]. Commands that change the
//! working tree refuse while `Core::has_unsaved_changes` and call `Core::reload` afterwards.
//! Credentials live in the secret store (`Core::secrets`).
//! Owner: wave 4, strand A1. Until then every command answers "unsupported".

use rt_app::AppError;
use rt_app::git::{
    GitBranch, GitCommit, GitConflict, GitDiff, GitIdentity, GitPullResult, GitResolution,
    GitStatus,
};
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::CollectionInfo;

type Result<T> = std::result::Result<T, CommandError>;

fn not_yet<T>() -> Result<T> {
    Err(AppError::Unsupported("Git is not implemented in the desktop app yet".into()).into())
}

#[tauri::command]
pub async fn git_status(core: State<'_, AppCore>) -> Result<GitStatus> {
    let _ = core;
    not_yet()
}

#[tauri::command]
pub async fn git_diff(core: State<'_, AppCore>, path: String) -> Result<GitDiff> {
    let _ = (core, path);
    not_yet()
}

#[tauri::command]
pub async fn git_commit(
    core: State<'_, AppCore>,
    message: String,
    paths: Vec<String>,
) -> Result<GitCommit> {
    let _ = (core, message, paths);
    not_yet()
}

#[tauri::command]
pub async fn git_log(core: State<'_, AppCore>, limit: u32) -> Result<Vec<GitCommit>> {
    let _ = (core, limit);
    not_yet()
}

#[tauri::command]
pub async fn git_branches(core: State<'_, AppCore>) -> Result<Vec<GitBranch>> {
    let _ = core;
    not_yet()
}

#[tauri::command]
pub async fn git_checkout(core: State<'_, AppCore>, branch: String) -> Result<()> {
    let _ = (core, branch);
    not_yet()
}

#[tauri::command]
pub async fn git_create_branch(
    core: State<'_, AppCore>,
    name: String,
    checkout: bool,
) -> Result<()> {
    let _ = (core, name, checkout);
    not_yet()
}

#[tauri::command]
pub async fn git_push(core: State<'_, AppCore>) -> Result<()> {
    let _ = core;
    not_yet()
}

#[tauri::command]
pub async fn git_pull(core: State<'_, AppCore>) -> Result<GitPullResult> {
    let _ = core;
    not_yet()
}

#[tauri::command]
pub async fn git_conflict(core: State<'_, AppCore>, path: String) -> Result<GitConflict> {
    let _ = (core, path);
    not_yet()
}

#[tauri::command]
pub async fn git_resolve(
    core: State<'_, AppCore>,
    path: String,
    resolution: GitResolution,
) -> Result<()> {
    let _ = (core, path, resolution);
    not_yet()
}

#[tauri::command]
pub async fn git_init(core: State<'_, AppCore>) -> Result<()> {
    let _ = core;
    not_yet()
}

/// Shows the folder picker, clones `url` into a new folder inside the picked one and opens
/// it; `None` if cancelled.
#[tauri::command]
pub async fn git_clone(
    app: tauri::AppHandle,
    core: State<'_, AppCore>,
    url: String,
) -> Result<Option<CollectionInfo>> {
    let _ = (app, core, url);
    not_yet()
}

#[tauri::command]
pub async fn git_identity(core: State<'_, AppCore>) -> Result<Option<GitIdentity>> {
    let _ = core;
    not_yet()
}

#[tauri::command]
pub async fn git_set_identity(core: State<'_, AppCore>, identity: GitIdentity) -> Result<()> {
    let _ = (core, identity);
    not_yet()
}

#[tauri::command]
pub async fn git_has_credentials(core: State<'_, AppCore>, host: String) -> Result<bool> {
    let _ = (core, host);
    not_yet()
}

#[tauri::command]
pub async fn git_set_credentials(
    core: State<'_, AppCore>,
    host: String,
    username: String,
    password: String,
) -> Result<()> {
    let _ = (core, host, username, password);
    not_yet()
}

#[tauri::command]
pub async fn git_delete_credentials(core: State<'_, AppCore>, host: String) -> Result<()> {
    let _ = (core, host);
    not_yet()
}
