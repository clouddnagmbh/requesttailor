// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Lint for the desktop app (`Backend.lint`, `rt_app::lint`). Wave 18 interface:
//! registered, "not implemented yet"; strand F answers them.

use rt_app::AppError;
use rt_app::lint::{LintReport, LintRule, LintSettings};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported("Lint is not implemented yet (wave 18, strand F).".into()).into())
}

#[tauri::command]
pub async fn lint_run(_app: tauri::AppHandle) -> Result<LintReport, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn lint_settings(_app: tauri::AppHandle) -> Result<LintSettings, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn lint_set_settings(
    _app: tauri::AppHandle,
    settings: LintSettings,
) -> Result<(), CommandError> {
    let _ = (settings,);
    not_yet()
}

#[tauri::command]
pub async fn lint_ignore(
    _app: tauri::AppHandle,
    path: String,
    rule: LintRule,
) -> Result<(), CommandError> {
    let _ = (path, rule);
    not_yet()
}
