// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin catalogue for the desktop app (`Backend.catalogue`, `rt_app::catalogue`). Wave 18 interface:
//! registered, "not implemented yet"; strand I answers them.

use rt_app::AppError;
use rt_app::catalogue::{
    CatalogueSource, CatalogueUpdate, InstallPlan, InstalledSource, SourceIndex,
};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "The plugin catalogue is not implemented yet (wave 18, strand I).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn catalogue_sources(
    _app: tauri::AppHandle,
) -> Result<Vec<CatalogueSource>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn catalogue_add_source(
    _app: tauri::AppHandle,
    url: String,
) -> Result<CatalogueSource, CommandError> {
    let _ = (url,);
    not_yet()
}

#[tauri::command]
pub async fn catalogue_remove_source(
    _app: tauri::AppHandle,
    url: String,
) -> Result<(), CommandError> {
    let _ = (url,);
    not_yet()
}

#[tauri::command]
pub async fn catalogue_refresh(_app: tauri::AppHandle) -> Result<Vec<SourceIndex>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn catalogue_plan(
    _app: tauri::AppHandle,
    source: String,
    id: String,
) -> Result<InstallPlan, CommandError> {
    let _ = (source, id);
    not_yet()
}

#[tauri::command]
pub async fn catalogue_install(
    _app: tauri::AppHandle,
    plan: InstallPlan,
) -> Result<InstalledSource, CommandError> {
    let _ = (plan,);
    not_yet()
}

#[tauri::command]
pub async fn catalogue_installed(
    _app: tauri::AppHandle,
) -> Result<Vec<InstalledSource>, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn catalogue_check_updates(
    _app: tauri::AppHandle,
) -> Result<Vec<CatalogueUpdate>, CommandError> {
    not_yet()
}
