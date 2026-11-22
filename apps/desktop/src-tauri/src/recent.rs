// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Recently opened collections, persisted in the app's data folder, so the app can reopen the
//! last one at start. Commands: `recent_collections`, `open_recent`.
//! Owner: wave 4, strand C. Until then nothing is remembered.

use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::CollectionInfo;

/// Mirrors `RecentCollection` in `apps/ui/src/commands/backend.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentCollection {
    #[serde(flatten)]
    pub info: CollectionInfo,
    /// Unix milliseconds.
    pub opened_at: i64,
}

pub struct Recent {}

impl Recent {
    /// Loads the list from `data_dir` (`None`: in memory only).
    pub fn new(data_dir: Option<&Path>) -> Self {
        let _ = data_dir;
        Self {}
    }

    /// Called by `Core::open` with every collection opened.
    pub fn opened(&self, info: &CollectionInfo) {
        let _ = info;
    }

    pub fn list(&self) -> Vec<RecentCollection> {
        Vec::new()
    }
}

#[tauri::command]
pub async fn recent_collections(
    core: State<'_, AppCore>,
) -> Result<Vec<RecentCollection>, CommandError> {
    Ok(core.recent.list())
}

/// Opens a recent collection by id (its canonical path). The desktop never needs consent, so
/// `interactive` is ignored; `None` if the folder is gone.
#[tauri::command]
pub async fn open_recent(
    core: State<'_, AppCore>,
    id: String,
    interactive: bool,
) -> Result<Option<CollectionInfo>, CommandError> {
    let _ = (core, id, interactive);
    Ok(None)
}
