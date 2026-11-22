// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Companion files of the open collection for extensions (wave 13): `companion_list`,
//! `companion_read`, `companion_stage` (`CompanionApi`), over `rt_app::companions`. Writes are
//! staged in the workspace and written by `save`.

use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn companion_list(
    core: State<'_, AppCore>,
    prefix: String,
) -> Result<Vec<String>, CommandError> {
    Ok(core.with_workspace(|ws, _| Ok(rt_app::companions::list(ws, &prefix)))?)
}

#[tauri::command]
pub async fn companion_read(
    core: State<'_, AppCore>,
    path: String,
) -> Result<Option<String>, CommandError> {
    Ok(core.with_workspace(|ws, _| Ok(rt_app::companions::read(ws, &path)))?)
}

#[tauri::command]
pub async fn companion_stage(
    core: State<'_, AppCore>,
    path: String,
    text: String,
) -> Result<(), CommandError> {
    Ok(core.with_workspace(|ws, _| rt_app::companions::stage(ws, &path, &text))?)
}
