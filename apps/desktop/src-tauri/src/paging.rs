// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paging for the desktop app (`Backend.paging`, `rt_app::paging`). Wave 18 interface:
//! registered, "not implemented yet"; strand C answers them.

use rt_app::AppError;
use rt_app::paging::{EffectivePaging, PagedResult, PagingRuleScope, PagingRun, PagingSpec};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported("Paging is not implemented yet (wave 18, strand C).".into()).into())
}

#[tauri::command]
pub async fn paging_rule(
    _app: tauri::AppHandle,
    path: String,
) -> Result<Option<EffectivePaging>, CommandError> {
    let _ = (path,);
    not_yet()
}

#[tauri::command]
pub async fn paging_set_rule(
    _app: tauri::AppHandle,
    path: String,
    spec: Option<PagingSpec>,
    scope: PagingRuleScope,
) -> Result<(), CommandError> {
    let _ = (path, spec, scope);
    not_yet()
}

#[tauri::command]
pub async fn paging_run(
    _app: tauri::AppHandle,
    run: PagingRun,
) -> Result<PagedResult, CommandError> {
    let _ = (run,);
    not_yet()
}
