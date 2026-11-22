// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic merge (`MergeApi`): the field conflicts of the merge in progress, recomputed from the index stages, and their resolution (`rt_app::merges`). The semantic pull itself is in `rt-git` (`merge.rs`).
//!
//! Wave 12, strand B. Interface placeholder: the commands are registered (`build.rs`,
//! `capabilities/default.json`, `generate_handler!` in `lib.rs`, `COMMANDS` in
//! `../src/backend.ts`) and refuse until the strand implements them here; the strand adds
//! their parameters.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet() -> CommandError {
    AppError::Unsupported("The semantic merge arrives in wave 12 (strand B).".into()).into()
}

#[tauri::command]
pub async fn merge_conflicts() -> Result<rt_app::merges::MergeState, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn merge_resolve() -> Result<(), CommandError> {
    Err(not_yet())
}
