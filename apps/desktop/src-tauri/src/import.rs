// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Import: convert a file (`rt_app::Imports`), then write the new collection into a folder
//! the user picks and open it. Commands: `import_analyze`, `import_write`, `import_discard`.
//! Owner: wave 4, strand B.

use rt_app::{AppError, ImportPlan};
use rt_format::import::ImportFormat;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::CollectionInfo;

#[tauri::command]
pub async fn import_analyze(
    core: State<'_, AppCore>,
    file_name: String,
    data: Vec<u8>,
    format: Option<ImportFormat>,
) -> Result<ImportPlan, CommandError> {
    let mut imports = core.imports.lock().unwrap_or_else(|p| p.into_inner());
    Ok(imports.analyze(&file_name, &data, format)?)
}

/// Shows the folder picker, writes plan `id` into a new folder `plan.folder_name` inside the
/// picked one and opens it; `None` if cancelled.
#[tauri::command]
pub async fn import_write(
    app: tauri::AppHandle,
    core: State<'_, AppCore>,
    id: u64,
) -> Result<Option<CollectionInfo>, CommandError> {
    let _ = (app, core, id);
    Err(AppError::Unsupported("writing an import is not implemented yet".into()).into())
}

#[tauri::command]
pub async fn import_discard(core: State<'_, AppCore>, id: u64) -> Result<(), CommandError> {
    core.imports
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .discard(id);
    Ok(())
}
