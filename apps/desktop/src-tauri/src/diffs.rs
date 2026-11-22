// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic diffs (`DiffApi`): the saved files, a commit or two revisions, read with `rt-git` (`Repo::tree_at`) and compared with `rt_app::diffs::diff`.
//!
//! Wave 12, strand A. Interface placeholder: the commands are registered (`build.rs`,
//! `capabilities/default.json`, `generate_handler!` in `lib.rs`, `COMMANDS` in
//! `../src/backend.ts`) and refuse until the strand implements them here; the strand adds
//! their parameters.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet() -> CommandError {
    AppError::Unsupported("The semantic diff arrives in wave 12 (strand A).".into()).into()
}

#[tauri::command]
pub async fn diff_working() -> Result<rt_app::diffs::CollectionDiff, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn diff_commit() -> Result<rt_app::diffs::CollectionDiff, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn diff_between() -> Result<rt_app::diffs::CollectionDiff, CommandError> {
    Err(not_yet())
}
