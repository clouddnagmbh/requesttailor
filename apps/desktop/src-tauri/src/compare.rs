// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Runs at a revision, run comparison and bisect (`CompareApi`, `rt_app::compare`, `rt_app::bisect`): the collection read with `Repo::tree_at`, sent like the runner (`runner.rs`) with the app's services.
//!
//! Wave 12, strand C. Interface placeholder: the commands are registered (`build.rs`,
//! `capabilities/default.json`, `generate_handler!` in `lib.rs`, `COMMANDS` in
//! `../src/backend.ts`) and refuse until the strand implements them here; the strand adds
//! their parameters.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet() -> CommandError {
    AppError::Unsupported("Branch compare and bisect arrives in wave 12 (strand C).".into()).into()
}

#[tauri::command]
pub async fn compare_run() -> Result<rt_app::compare::RevisionRun, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn compare_revisions() -> Result<rt_app::compare::RunComparison, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn compare_bisect() -> Result<rt_app::bisect::BisectOutcome, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn compare_cancel() -> Result<(), CommandError> {
    Err(not_yet())
}
