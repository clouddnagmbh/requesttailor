// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Recently opened collections, persisted in the app's data folder, so the app can reopen the
//! last one at start. Commands: `recent_collections`, `open_recent`.
//!
//! - The list is `recent-collections.json` in the data folder: at most [`MAX_RECENT`]
//!   entries, most recent first. A missing or unreadable file is an empty list; failing to
//!   write it never fails an open.
//! - `open_recent` only opens folders that are on the list: the webview names a list entry,
//!   it cannot open arbitrary paths. A folder that is gone is dropped from the list.
//!
//! Measurement mode (`perf/README.md`, never active in normal use): when the environment
//! variable `RT_PERF_DIR` is set, the list is read from and written to that folder instead
//! of the data folder, and the app appends timing marks to `RT_PERF_DIR/marks.log`
//! (`<mark> <ms since RT_PERF_T0>` per line). Nothing is sent anywhere.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rt_app::{AppError, SecretStore};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{CollectionInfo, Core};

/// Entries kept in the list.
pub const MAX_RECENT: usize = 10;

/// File name of the list in the data folder.
pub const FILE_NAME: &str = "recent-collections.json";

/// Mirrors `RecentCollection` in `apps/ui/src/commands/backend.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentCollection {
    #[serde(flatten)]
    pub info: CollectionInfo,
    /// Unix milliseconds.
    pub opened_at: i64,
}

/// One entry as stored in the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    id: String,
    name: String,
    location: String,
    opened_at: i64,
}

#[derive(Serialize, Deserialize)]
struct StoredFile {
    version: u32,
    collections: Vec<Stored>,
}

pub struct Recent {
    file: Option<PathBuf>,
    entries: Mutex<Vec<Stored>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Recent {
    /// Loads the list from `data_dir` (`None`: in memory only). In measurement mode
    /// (`RT_PERF_DIR` set) the list lives in that folder instead.
    pub fn new(data_dir: Option<&Path>) -> Self {
        // The app's setup creates the core (and with it this list) before the window opens.
        perf::mark_once("core-ready");
        let dir = perf::dir().or_else(|| data_dir.map(Path::to_path_buf));
        Self::in_dir(dir.as_deref())
    }

    /// Like [`Recent::new`], without the measurement-mode override (tests).
    pub fn in_dir(dir: Option<&Path>) -> Self {
        let file = dir.map(|d| d.join(FILE_NAME));
        let entries = file.as_deref().map(load).unwrap_or_default();
        Self {
            file,
            entries: Mutex::new(entries),
        }
    }

    /// Called by `Core::open` with every collection opened: moves it to the top.
    pub fn opened(&self, info: &CollectionInfo) {
        let mut entries = lock(&self.entries);
        entries.retain(|e| e.id != info.id);
        entries.insert(
            0,
            Stored {
                id: info.id.clone(),
                name: info.name.clone(),
                location: info.location.clone(),
                opened_at: now_ms(),
            },
        );
        entries.truncate(MAX_RECENT);
        self.persist(&entries);
    }

    /// Drops `id` from the list (its folder is gone).
    pub fn forget(&self, id: &str) {
        let mut entries = lock(&self.entries);
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() != before {
            self.persist(&entries);
        }
    }

    pub fn list(&self) -> Vec<RecentCollection> {
        lock(&self.entries)
            .iter()
            .map(|e| RecentCollection {
                info: CollectionInfo {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    location: e.location.clone(),
                },
                opened_at: e.opened_at,
            })
            .collect()
    }

    fn contains(&self, id: &str) -> bool {
        lock(&self.entries).iter().any(|e| e.id == id)
    }

    fn persist(&self, entries: &[Stored]) {
        let Some(file) = &self.file else { return };
        if let Err(e) = save(file, entries) {
            eprintln!(
                "RequestTailor: cannot write the recent collections to {}: {e}",
                file.display()
            );
        }
    }
}

fn load(file: &Path) -> Vec<Stored> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    match serde_json::from_str::<StoredFile>(&text) {
        Ok(f) if f.version == 1 => {
            let mut v = f.collections;
            v.truncate(MAX_RECENT);
            v
        }
        _ => Vec::new(),
    }
}

/// Writes a temporary file next to `file` and renames it, so a crash never leaves half a list.
fn save(file: &Path, entries: &[Stored]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(&StoredFile {
        version: 1,
        collections: entries.to_vec(),
    })
    .map_err(std::io::Error::other)?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, file)
}

/// Opens the recent collection `id`. `None` if `id` is not on the list or its folder is gone
/// (then it is dropped from the list). Errors reading an existing folder are returned.
/// Wave 14: entries are the folders the user opened or added (roots); if `id` is a root of
/// the set remembered last, that whole set is restored with its active collection.
pub fn open_recent_in<S: SecretStore>(
    core: &Core<S>,
    id: &str,
) -> Result<Option<CollectionInfo>, AppError> {
    if !core.recent.contains(id) {
        return Ok(None);
    }
    let root = PathBuf::from(id);
    if !root.is_dir() {
        core.recent.forget(id);
        return Ok(None);
    }
    // Wave 14: a folder of the remembered collection set brings the whole set back.
    if let Some(info) = core.restore(&root)? {
        return Ok(Some(info));
    }
    core.open(&root).map(Some)
}

#[tauri::command]
pub async fn recent_collections(
    core: State<'_, AppCore>,
) -> Result<Vec<RecentCollection>, CommandError> {
    // The UI asks for the list once at start, right after the shell's first frame
    // (`apps/ui/src/recent/startup.ts`): the cold-start measurement's end point.
    perf::mark_once("first-frame");
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
    let _ = interactive;
    let opened = open_recent_in(core.inner(), &id)?;
    if opened.is_some() {
        perf::mark_once("collection-open");
    }
    Ok(opened)
}

/// Measurement mode (see the module docs). Inactive unless `RT_PERF_DIR` is set.
pub mod perf {
    use super::*;

    use std::collections::HashSet;
    use std::sync::OnceLock;

    /// `RT_PERF_DIR`, if set and not empty.
    pub fn dir() -> Option<PathBuf> {
        std::env::var_os("RT_PERF_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }

    /// Milliseconds since `RT_PERF_T0` (Unix milliseconds, set by the measuring script just
    /// before it starts the app), or `None` if it is not set.
    fn elapsed_ms() -> Option<f64> {
        let t0: f64 = std::env::var("RT_PERF_T0").ok()?.trim().parse().ok()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
        Some(now.as_secs_f64() * 1000.0 - t0)
    }

    /// Appends `<name> <ms>` to `RT_PERF_DIR/marks.log` the first time it is called with
    /// `name`. Does nothing outside measurement mode.
    pub fn mark_once(name: &str) {
        static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        let Some(dir) = dir() else { return };
        if !lock(SEEN.get_or_init(Default::default)).insert(name.to_owned()) {
            return;
        }
        let Some(ms) = elapsed_ms() else { return };
        let line = format!("{name} {ms:.1}\n");
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("marks.log"))
            .and_then(|mut f| f.write_all(line.as_bytes()));
        if let Err(e) = written {
            eprintln!("RequestTailor: cannot write a perf mark: {e}");
        }
    }
}
