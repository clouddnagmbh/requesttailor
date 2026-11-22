// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Request history of the desktop app: recorded on every send (`Core::send`), persisted as
//! JSON Lines in the app's data folder. Commands: `history_list`, `history_clear`.
//! Owner: wave 4, strand E. Until then nothing is recorded.

use std::path::Path;
use std::sync::Mutex;

use rt_app::{AppError, History, HistoryEntry};
use rt_core::send::SendReport;
use tauri::State;

use crate::commands::{AppCore, CommandError};

pub struct DesktopHistory {
    history: Mutex<History>,
}

impl DesktopHistory {
    /// Loads the history from `data_dir` (`None`: in memory only).
    pub fn new(data_dir: Option<&Path>) -> Self {
        let _ = data_dir;
        Self {
            history: Mutex::new(History::new()),
        }
    }

    /// Called by `Core::send` after every send. Never fails the send.
    pub fn record(&self, collection: &str, name: &str, method: &str, report: &SendReport) {
        let _ = (&self.history, collection, name, method, report);
    }

    pub fn list(&self, collection: &str, query: &str, limit: usize) -> Vec<HistoryEntry> {
        let _ = (collection, query, limit);
        Vec::new()
    }

    pub fn clear(&self, collection: &str) -> Result<(), AppError> {
        let _ = collection;
        Ok(())
    }
}

#[tauri::command]
pub async fn history_list(
    core: State<'_, AppCore>,
    query: String,
    limit: u32,
) -> Result<Vec<HistoryEntry>, CommandError> {
    let Some(id) = core.collection_id() else {
        return Ok(Vec::new());
    };
    Ok(core.history.list(&id, &query, limit as usize))
}

#[tauri::command]
pub async fn history_clear(core: State<'_, AppCore>) -> Result<(), CommandError> {
    let Some(id) = core.collection_id() else {
        return Ok(());
    };
    Ok(core.history.clear(&id)?)
}
