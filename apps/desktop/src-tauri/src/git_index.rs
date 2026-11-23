// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The index as building blocks (`GitApi.indexDiff`, `stage`, `unstage`, `stagePatch`,
//! `unstagePatch`, `discardPatch`, `commitStaged`; `rt_app::git::index`) and signature checks
//! (`GitApi.verify`, `rt_app::git::signing`). Wave 15 interface: registered, "not implemented
//! yet"; strand F answers them from `rt-git` and passes a distribution's `rt_git::sign::Signing`
//! (managed state, read with `try_state`) to every commit the app writes.

use rt_app::AppError;
use rt_app::git::GitCommit;
use rt_app::git::index::{GitIndexDiff, GitPatchRequest, GitPatchSide, GitStagedCommit};
use rt_app::git::signing::GitSignatureCheck;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported("Index operations arrive with wave 15 (strand F).".into()).into())
}

#[tauri::command]
pub async fn git_index_diff(
    _app: tauri::AppHandle,
    path: String,
    side: GitPatchSide,
) -> Result<GitIndexDiff, CommandError> {
    let _ = (path, side);
    not_yet()
}

#[tauri::command]
pub async fn git_stage(_app: tauri::AppHandle, paths: Vec<String>) -> Result<(), CommandError> {
    let _ = paths;
    not_yet()
}

#[tauri::command]
pub async fn git_unstage(_app: tauri::AppHandle, paths: Vec<String>) -> Result<(), CommandError> {
    let _ = paths;
    not_yet()
}

#[tauri::command]
pub async fn git_stage_patch(
    _app: tauri::AppHandle,
    patch: GitPatchRequest,
) -> Result<(), CommandError> {
    let _ = patch;
    not_yet()
}

#[tauri::command]
pub async fn git_unstage_patch(
    _app: tauri::AppHandle,
    patch: GitPatchRequest,
) -> Result<(), CommandError> {
    let _ = patch;
    not_yet()
}

#[tauri::command]
pub async fn git_discard_patch(
    _app: tauri::AppHandle,
    patch: GitPatchRequest,
) -> Result<(), CommandError> {
    let _ = patch;
    not_yet()
}

#[tauri::command]
pub async fn git_commit_staged(
    _app: tauri::AppHandle,
    commit: GitStagedCommit,
) -> Result<GitCommit, CommandError> {
    let _ = commit;
    not_yet()
}

#[tauri::command]
pub async fn git_verify(
    _app: tauri::AppHandle,
    oids: Vec<String>,
) -> Result<Vec<GitSignatureCheck>, CommandError> {
    let _ = oids;
    not_yet()
}
