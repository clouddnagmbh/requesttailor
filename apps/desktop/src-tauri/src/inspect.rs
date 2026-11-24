// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Response checks for the desktop app (`Backend.inspect`, `rt_app::inspect`). Wave 18 interface:
//! registered, "not implemented yet"; strand B answers them.

use rt_app::AppError;
use rt_app::inspect::{CheckScope, FoundToken, InspectTarget, JwtInfo, Observation};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(
        AppError::Unsupported("Response checks is not implemented yet (wave 18, strand B).".into())
            .into(),
    )
}

#[tauri::command]
pub async fn inspect_jwt(_app: tauri::AppHandle, token: String) -> Result<JwtInfo, CommandError> {
    let _ = (token,);
    not_yet()
}

#[tauri::command]
pub async fn inspect_tokens(
    _app: tauri::AppHandle,
    target: InspectTarget,
) -> Result<Vec<FoundToken>, CommandError> {
    let _ = (target,);
    not_yet()
}

#[tauri::command]
pub async fn inspect_checks(
    _app: tauri::AppHandle,
    target: InspectTarget,
) -> Result<Vec<Observation>, CommandError> {
    let _ = (target,);
    not_yet()
}

#[tauri::command]
pub async fn inspect_ignore_check(
    _app: tauri::AppHandle,
    id: String,
    scope: CheckScope,
) -> Result<(), CommandError> {
    let _ = (id, scope);
    not_yet()
}
