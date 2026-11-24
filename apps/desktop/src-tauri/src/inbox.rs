// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The webhook inbox for the desktop app (`Backend.inbox`, `rt_app::inbox`). Wave 18 interface:
//! registered, "not implemented yet"; strand E answers them.

use rt_app::AppError;
use rt_app::inbox::{CapturedRequest, InboxDraft, InboxPage, InboxSettings, InboxStatus};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "The webhook inbox is not implemented yet (wave 18, strand E).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn inbox_settings(_app: tauri::AppHandle) -> Result<InboxSettings, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn inbox_set_settings(
    _app: tauri::AppHandle,
    settings: InboxSettings,
) -> Result<(), CommandError> {
    let _ = (settings,);
    not_yet()
}

#[tauri::command]
pub async fn inbox_start(_app: tauri::AppHandle) -> Result<InboxStatus, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn inbox_stop(_app: tauri::AppHandle) -> Result<InboxStatus, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn inbox_status(_app: tauri::AppHandle) -> Result<InboxStatus, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn inbox_events(_app: tauri::AppHandle, since: u64) -> Result<InboxPage, CommandError> {
    let _ = (since,);
    not_yet()
}

#[tauri::command]
pub async fn inbox_get(
    _app: tauri::AppHandle,
    id: u64,
) -> Result<Option<CapturedRequest>, CommandError> {
    let _ = (id,);
    not_yet()
}

#[tauri::command]
pub async fn inbox_clear(_app: tauri::AppHandle) -> Result<(), CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn inbox_make_request(
    _app: tauri::AppHandle,
    id: u64,
) -> Result<InboxDraft, CommandError> {
    let _ = (id,);
    not_yet()
}
