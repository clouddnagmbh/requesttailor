// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `Backend.folderScripts` (wave 16): the scripts of the collection (`folder` is `""`) or of
//! a folder, and replacing them (unsaved, with the read-back check of `rt-format`).

use rt_core::model::Script;
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn folder_scripts_get(
    core: State<'_, AppCore>,
    path: String,
) -> Result<Vec<Script>, CommandError> {
    Ok(core.with_workspace(|ws, _| ws.folder_scripts(&path))?)
}

#[tauri::command]
pub async fn folder_scripts_set(
    core: State<'_, AppCore>,
    path: String,
    scripts: Vec<Script>,
) -> Result<(), CommandError> {
    Ok(core.with_workspace(|ws, _| ws.set_folder_scripts(&path, &scripts))?)
}
