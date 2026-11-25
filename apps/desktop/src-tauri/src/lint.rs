// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Lint for the desktop app (`Backend.lint`, `rt_app::lint`), answered from the active
//! collection's model in memory: unsaved edits are linted, nothing is read from disk or sent,
//! settings and "ignore" are staged in `opencollection.yml` (unsaved).

use rt_app::lint::{self, LintReport, LintRule, LintSettings};
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn lint_run(core: State<'_, AppCore>) -> Result<LintReport, CommandError> {
    Ok(core.with_workspace(|ws, _| Ok(lint::run(ws)))?)
}

#[tauri::command]
pub async fn lint_settings(core: State<'_, AppCore>) -> Result<LintSettings, CommandError> {
    Ok(core.with_workspace(|ws, _| Ok(lint::settings(ws)))?)
}

#[tauri::command]
pub async fn lint_set_settings(
    core: State<'_, AppCore>,
    settings: LintSettings,
) -> Result<(), CommandError> {
    Ok(core.with_workspace(|ws, _| lint::set_settings(ws, &settings))?)
}

#[tauri::command]
pub async fn lint_ignore(
    core: State<'_, AppCore>,
    path: String,
    rule: LintRule,
) -> Result<(), CommandError> {
    Ok(core.with_workspace(|ws, _| lint::ignore(ws, &path, rule))?)
}
