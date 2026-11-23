// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The Git wizards' backend (`GitApi.plan`, `apply`, `undo`, remotes, stashes;
//! `rt_app::git::actions`). Wave 14, strand G: natively in `rt-git` (gitoxide), backups under
//! `refs/requesttailor/backup/`, journal in `.git/requesttailor/actions.jsonl`; after a change
//! of the working tree every open collection of the repository reopens. Placeholders until then.

use rt_app::AppError;
use rt_app::git::actions::{
    GitAction, GitActionResult, GitPlan, GitRemote, GitStash, GitUndoEntry,
};

use crate::commands::CommandError;

fn later<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported("Git wizards arrive in wave 14 (strand G)".into()).into())
}

#[tauri::command]
pub async fn git_remotes() -> Result<Vec<GitRemote>, CommandError> {
    later()
}

#[tauri::command]
pub async fn git_stashes() -> Result<Vec<GitStash>, CommandError> {
    later()
}

#[tauri::command]
pub async fn git_plan(action: GitAction) -> Result<GitPlan, CommandError> {
    let _ = action;
    later()
}

#[tauri::command]
pub async fn git_apply(action: GitAction) -> Result<GitActionResult, CommandError> {
    let _ = action;
    later()
}

#[tauri::command]
pub async fn git_last_action() -> Result<Option<GitUndoEntry>, CommandError> {
    Ok(None)
}

#[tauri::command]
pub async fn git_undo(id: String) -> Result<GitActionResult, CommandError> {
    let _ = id;
    later()
}
