// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Comparing environments for the desktop app (`Backend.envCompare`, `rt_app::envcompare`). Wave 18 interface:
//! registered, "not implemented yet"; strand A answers them.

use rt_app::AppError;
use rt_app::envcompare::{EnvCompareRequest, EnvComparison};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "Comparing environments is not implemented yet (wave 18, strand A).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn envcompare_run(
    _app: tauri::AppHandle,
    request: EnvCompareRequest,
) -> Result<EnvComparison, CommandError> {
    let _ = (request,);
    not_yet()
}
