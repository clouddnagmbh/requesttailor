// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The test-data journal for the desktop app (`Backend.journal`, `rt_app::journal`). Wave 19 interface:
//! registered, "not implemented yet"; strand C answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "The test-data journal is not implemented yet (wave 19, strand C).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn journal_settings(
    _app: tauri::AppHandle,
) -> Result<rt_app::journal::JournalSettings, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_set_settings(
    _app: tauri::AppHandle,
    _settings: rt_app::journal::JournalSettings,
) -> Result<(), CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_entries(
    _app: tauri::AppHandle,
    _query: rt_app::journal::JournalQuery,
) -> Result<rt_app::journal::JournalPage, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_clear(_app: tauri::AppHandle) -> Result<(), CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_plan_manual(
    _app: tauri::AppHandle,
    _revert: rt_app::journal::ManualRevert,
) -> Result<rt_app::journal::RevertPlan, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_plan_compensations(
    _app: tauri::AppHandle,
    _items: Vec<rt_app::journal::CompensationItem>,
) -> Result<rt_app::journal::RevertPlan, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_execute(
    _app: tauri::AppHandle,
    _plan: rt_app::journal::RevertPlan,
    _options: rt_app::journal::RevertOptions,
) -> Result<rt_app::journal::RevertOutcome, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_plugin_rules(
    _app: tauri::AppHandle,
) -> Result<Vec<rt_app::journal::PluginCompensationRule>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_pre_read(
    _app: tauri::AppHandle,
    _request: rt_core::model::RequestSpec,
    _environment: Option<String>,
) -> Result<rt_app::journal::PreRead, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn journal_states(
    _app: tauri::AppHandle,
    _entry_ids: Vec<u64>,
) -> Result<Vec<rt_app::journal::EntryStates>, CommandError> {
    not_yet()
}
