// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The access check for the desktop app (`Backend.access`, `rt_app::access`). Wave 19 interface:
//! registered, "not implemented yet"; strand D answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "The access check is not implemented yet (wave 19, strand D).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn access_check(
    _app: tauri::AppHandle,
    _path: String,
    _profile: rt_app::access::AccessProfile,
    _options: rt_app::access::AccessOptions,
) -> Result<rt_app::access::AccessCheck, CommandError> {
    not_yet()
}
