// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Request history of the desktop app: recorded on every send (`Core::send`), persisted as
//! JSON Lines in the app's data folder. Commands: `history_list`, `history_clear`.
//!
//! - `history.jsonl` holds one `HistoryEntry` per line, oldest first. A send appends one line;
//!   pruning (beyond `HISTORY_LIMIT`) and clearing rewrite the file (temporary file, then
//!   rename, so a crash never leaves half a history).
//! - Lines that do not parse (a crash mid-append, a hand edit) are skipped when loading, and
//!   the file is rewritten without them, so the next append starts on a fresh line.
//! - Entries hold no secret values (see `rt_app::history`); the file is as safe to keep as
//!   the collection files.
//! - Without a data folder (`None`, tests) the history lives in memory only.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rt_app::history::HISTORY_LIMIT;
use rt_app::{AppError, History, HistoryEntry};
use rt_core::send::SendReport;
use tauri::State;

use crate::commands::{AppCore, CommandError};

/// File name of the history in the app's data folder.
pub const HISTORY_FILE: &str = "history.jsonl";

pub struct DesktopHistory {
    history: Mutex<History>,
    /// `None`: in memory only.
    file: Option<PathBuf>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl DesktopHistory {
    /// Loads the history from `data_dir` (`None`: in memory only). A missing or unreadable
    /// file is an empty history; broken lines are dropped.
    pub fn new(data_dir: Option<&Path>) -> Self {
        let file = data_dir.map(|d| d.join(HISTORY_FILE));
        let (history, clean) = match &file {
            Some(path) => load(path),
            None => (History::new(), true),
        };
        let this = Self {
            history: Mutex::new(history),
            file,
        };
        if !clean {
            let history = this.lock();
            if let Err(e) = this.rewrite(&history) {
                eprintln!("RequestTailor: cannot repair the history file: {e}");
            }
        }
        this
    }

    fn lock(&self) -> MutexGuard<'_, History> {
        self.history
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Called by `Core::send` after every send. Never fails the send: a write error is
    /// logged and the entry stays in memory.
    pub fn record(&self, collection: &str, name: &str, method: &str, report: &SendReport) {
        let mut history = self.lock();
        let entry = history.record(collection, name, method, now_ms(), report);
        let dropped = history.prune();
        let written = if dropped.is_empty() {
            self.append(&entry)
        } else {
            self.rewrite(&history)
        };
        if let Err(e) = written {
            eprintln!("RequestTailor: cannot write the history: {e}");
        }
    }

    pub fn list(&self, collection: &str, query: &str, limit: usize) -> Vec<HistoryEntry> {
        self.lock()
            .list(collection, query, limit.min(HISTORY_LIMIT))
    }

    pub fn clear(&self, collection: &str) -> Result<(), AppError> {
        let mut history = self.lock();
        history.clear(collection);
        self.rewrite(&history)
            .map_err(|e| AppError::Shell(format!("cannot clear the history: {e}")))
    }

    fn append(&self, entry: &HistoryEntry) -> io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut line = serde_json::to_string(entry).map_err(io::Error::other)?;
        line.push('\n');
        // One write call per line: a crash leaves at most one partial last line.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(line.as_bytes())
    }

    fn rewrite(&self, history: &History) -> io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut text = String::new();
        for entry in history.entries() {
            text.push_str(&serde_json::to_string(entry).map_err(io::Error::other)?);
            text.push('\n');
        }
        let tmp = path.with_extension("jsonl.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }
}

/// The history in `path` and whether the file is clean (every line parsed, ends with a line
/// break). A missing file is clean.
fn load(path: &Path) -> (History, bool) {
    let text = match fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (History::new(), true),
        Err(e) => {
            eprintln!("RequestTailor: cannot read the history: {e}");
            return (History::new(), true);
        }
    };
    let mut clean = text.is_empty() || text.ends_with('\n');
    let mut entries = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<HistoryEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => clean = false,
        }
    }
    let count = entries.len();
    let history = History::load(entries);
    // Pruned or deduplicated on load: write the file as it is now.
    clean &= history.entries().len() == count;
    (history, clean)
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
