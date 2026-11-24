// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The script debugger for the desktop app (`Backend.debugger`, `rt_script::debug`). Wave 19 interface:
//! registered, "not implemented yet"; strand Y answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "The script debugger is not implemented yet (wave 19, strand Y).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn debugger_start(
    _app: tauri::AppHandle,
    _target: rt_script::debug::DebugTarget,
) -> Result<rt_script::debug::DebugState, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn debugger_command(
    _app: tauri::AppHandle,
    _session: u32,
    _command: rt_script::debug::DebugCommand,
) -> Result<rt_script::debug::DebugState, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn debugger_state(
    _app: tauri::AppHandle,
    _session: u32,
) -> Result<rt_script::debug::DebugState, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn debugger_stop(_app: tauri::AppHandle, _session: u32) -> Result<(), CommandError> {
    not_yet()
}
