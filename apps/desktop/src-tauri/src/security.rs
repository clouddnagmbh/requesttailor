// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Passive security checks for the desktop app (`Backend.security`, `rt_app::security`). Wave 19 interface:
//! registered, "not implemented yet"; strand R answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "Passive security checks is not implemented yet (wave 19, strand R).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn security_check(
    _app: tauri::AppHandle,
    _target: rt_app::security::SecurityTarget,
) -> Result<Vec<rt_app::security::SecurityFinding>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn security_rules(
    _app: tauri::AppHandle,
) -> Result<Vec<rt_app::security::SecurityRule>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn security_settings(
    _app: tauri::AppHandle,
) -> Result<rt_app::security::SecuritySettings, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn security_set_settings(
    _app: tauri::AppHandle,
    _settings: rt_app::security::SecuritySettings,
) -> Result<(), CommandError> {
    not_yet()
}
