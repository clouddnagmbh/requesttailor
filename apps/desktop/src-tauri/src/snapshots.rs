// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests for the desktop app (`Backend.snapshots`, `rt_app::snapshots`). Wave 17
//! interface: registered, "not implemented yet"; strand B answers them from the active
//! collection (companion files through `rt_app::companions`, the assertion through the
//! request).

use rt_app::AppError;
use rt_app::snapshots::{FreezeOptions, Snapshot, SnapshotCheck, SnapshotEntry, SnapshotFreeze};

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(
        AppError::Unsupported("Snapshots are not implemented yet (wave 17, strand B).".into())
            .into(),
    )
}

#[tauri::command]
pub async fn snapshots_read(
    _app: tauri::AppHandle,
    path: String,
) -> Result<Option<Snapshot>, CommandError> {
    let _ = path;
    not_yet()
}

#[tauri::command]
pub async fn snapshots_freeze(
    _app: tauri::AppHandle,
    path: String,
    response_id: u64,
    options: FreezeOptions,
) -> Result<SnapshotFreeze, CommandError> {
    let _ = (path, response_id, options);
    not_yet()
}

#[tauri::command]
pub async fn snapshots_set_ignore(
    _app: tauri::AppHandle,
    path: String,
    ignore: Vec<String>,
) -> Result<Snapshot, CommandError> {
    let _ = (path, ignore);
    not_yet()
}

#[tauri::command]
pub async fn snapshots_check(
    _app: tauri::AppHandle,
    path: String,
    response_id: u64,
) -> Result<SnapshotCheck, CommandError> {
    let _ = (path, response_id);
    not_yet()
}

#[tauri::command]
pub async fn snapshots_list(_app: tauri::AppHandle) -> Result<Vec<SnapshotEntry>, CommandError> {
    not_yet()
}
