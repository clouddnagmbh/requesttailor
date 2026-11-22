// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OpenAPI contract (`ContractApi`, `rt_app::contract`): the spec from the work tree or `Repo::show`, check, spec changes between revisions, changelog.
//!
//! Wave 12, strand F. Interface placeholder: the commands are registered (`build.rs`,
//! `capabilities/default.json`, `generate_handler!` in `lib.rs`, `COMMANDS` in
//! `../src/backend.ts`) and refuse until the strand implements them here; the strand adds
//! their parameters.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet() -> CommandError {
    AppError::Unsupported("The contract check arrives in wave 12 (strand F).".into()).into()
}

#[tauri::command]
pub async fn contract_spec() -> Result<Option<rt_app::contract::ContractSpec>, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn contract_check() -> Result<rt_app::contract::ContractReport, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn contract_changes() -> Result<rt_app::contract::SpecChanges, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn contract_changelog() -> Result<String, CommandError> {
    Err(not_yet())
}
