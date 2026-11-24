// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Egress for extensions for the desktop app (`Backend.egress`, `rt_app::egress`). Wave 19 interface:
//! registered, "not implemented yet"; strand B answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "Egress for extensions is not implemented yet (wave 19, strand B).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn egress_hosts(
    _app: tauri::AppHandle,
) -> Result<Vec<rt_app::egress::EgressHost>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn egress_allow(
    _app: tauri::AppHandle,
    _host: String,
    _port: Option<u16>,
    _extension: String,
    _purpose: String,
) -> Result<rt_app::egress::EgressHost, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn egress_remove(
    _app: tauri::AppHandle,
    _host: String,
    _port: Option<u16>,
) -> Result<(), CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn egress_request(
    _app: tauri::AppHandle,
    _request: rt_app::egress::EgressRequest,
) -> Result<rt_app::egress::EgressResponse, CommandError> {
    not_yet()
}
