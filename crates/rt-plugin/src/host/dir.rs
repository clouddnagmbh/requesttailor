// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Installed plugins on disk, shared by the desktop app and the CLI (not in the browser,
//! which keeps them in its own storage).
//!
//! The plugin folder is `plugins/` in the desktop app's data folder:
//!
//! | OS | Folder |
//! | --- | --- |
//! | macOS | `~/Library/Application Support/dev.requesttailor.desktop/plugins` |
//! | Linux | `$XDG_DATA_HOME/dev.requesttailor.desktop/plugins` (default `~/.local/share/…`) |
//! | Windows | `%APPDATA%\dev.requesttailor.desktop\plugins` |
//!
//! `RT_DATA_DIR` replaces the data folder for the CLI (`$RT_DATA_DIR/plugins`). Inside:
//! - `<id>.wasm`: the module as installed, unchanged;
//! - `plugins.json`: installation order, the granted permissions, enabled or not, and, for a
//!   plugin installed from a catalogue source (wave 18), the source and the SHA-256 of the module;
//! - `sources.json`: the catalogue sources the user added (written by the app layer through
//!   [`PluginDir::read_aux`] and [`PluginDir::write_aux`]; nothing here reads it);
//! - `storage/<id>.json`: the plugin's `storage` (one JSON object per plugin).
//!
//! Writes go to a temporary file first and are renamed into place, so a crash never leaves
//! a half-written module or state file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::services::PluginStorage;
use super::{Plugin, PluginError};
use crate::api::{Manifest, Permissions};
use rt_core::services::LocalBoxFuture;

/// The data folder of the desktop app (its Tauri identifier).
pub const APP_IDENTIFIER: &str = "dev.requesttailor.desktop";
const STATE_FILE: &str = "plugins.json";

/// The desktop app's data folder on this OS, or `$RT_DATA_DIR` if set.
pub fn data_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = env("RT_DATA_DIR") {
        return Some(dir);
    }
    let base = if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        env("HOME").map(|h| h.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local").join("share")))
    }?;
    Some(base.join(APP_IDENTIFIER))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    plugins: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    id: String,
    version: String,
    granted: Permissions,
    enabled: bool,
    /// Where the module was installed from (a catalogue source), if it was (wave 18).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    /// SHA-256 of the installed module (lower-case hex), with `source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

/// Where an installed module came from: recorded by [`PluginDir::install_from`] (wave 18, the
/// plugin catalogue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The catalogue source the user added.
    pub source: String,
    /// SHA-256 of the module (lower-case hex).
    pub sha256: String,
}

/// One installed plugin as recorded in `plugins.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRecord {
    pub id: String,
    pub version: String,
    pub granted: Permissions,
    pub enabled: bool,
    /// `None`: installed from a file.
    pub origin: Option<Origin>,
}

/// The plugin folder.
#[derive(Debug, Clone)]
pub struct PluginDir {
    dir: PathBuf,
}

fn io(e: impl std::fmt::Display) -> PluginError {
    PluginError::Failed {
        id: "plugins".into(),
        message: e.to_string(),
    }
}

/// Writes `bytes` to `path` through a temporary file in the same folder.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// A plain file name (no folders), so a caller cannot reach outside the plugin folder.
fn aux_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', '\0'])
}

impl PluginDir {
    /// The plugin folder inside `data_dir` (it is created on the first install).
    pub fn in_data_dir(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("plugins"),
        }
    }

    /// The folder the desktop app and the CLI share (see the module documentation).
    pub fn shared() -> Option<Self> {
        data_dir().map(|d| Self::in_data_dir(&d))
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn module_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.wasm"))
    }

    fn state(&self) -> State {
        std::fs::read(self.dir.join(STATE_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &State) -> Result<(), PluginError> {
        let json = serde_json::to_vec_pretty(state).map_err(io)?;
        write_atomic(&self.dir.join(STATE_FILE), &json).map_err(io)
    }

    /// The installed plugins in installation order. A plugin that does not load any more
    /// (file gone or damaged) is left out and reported in the second list.
    pub fn load(&self) -> (Vec<Plugin>, Vec<(String, PluginError)>) {
        let mut plugins = Vec::new();
        let mut failed = Vec::new();
        for entry in self.state().plugins {
            let loaded = std::fs::read(self.module_path(&entry.id))
                .map_err(io)
                .and_then(|wasm| Plugin::compile(&wasm, &entry.granted, entry.enabled))
                .and_then(|p| {
                    if p.id() == entry.id {
                        Ok(p)
                    } else {
                        Err(PluginError::Invalid(format!(
                            "{}.wasm contains plugin {}",
                            entry.id,
                            p.id()
                        )))
                    }
                });
            match loaded {
                Ok(p) => plugins.push(p),
                Err(e) => failed.push((entry.id, e)),
            }
        }
        (plugins, failed)
    }

    /// Installs or updates a plugin and grants what its manifest requests (the shell asked
    /// the user first). An update keeps the plugin's place in the order and whether it was
    /// enabled; a new plugin is enabled.
    pub fn install(&self, wasm: &[u8]) -> Result<Plugin, PluginError> {
        self.install_from(wasm, None)
    }

    /// [`PluginDir::install`], recording where the module came from (`None`: a file; an
    /// earlier origin of an updated plugin is dropped then).
    pub fn install_from(&self, wasm: &[u8], origin: Option<Origin>) -> Result<Plugin, PluginError> {
        let manifest: Manifest = super::PluginHost::inspect(wasm)?;
        let mut state = self.state();
        let enabled = state
            .plugins
            .iter()
            .find(|e| e.id == manifest.id)
            .is_none_or(|e| e.enabled);
        let plugin = Plugin::compile(wasm, &manifest.permissions, enabled)?;
        write_atomic(&self.module_path(&manifest.id), wasm).map_err(io)?;
        let entry = Entry {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            granted: manifest.permissions.clone(),
            enabled,
            source: origin.as_ref().map(|o| o.source.clone()),
            sha256: origin.map(|o| o.sha256),
        };
        match state.plugins.iter_mut().find(|e| e.id == manifest.id) {
            Some(e) => *e = entry,
            None => state.plugins.push(entry),
        }
        self.save_state(&state)?;
        Ok(plugin)
    }

    /// What is installed, in installation order, as recorded (nothing is compiled).
    pub fn records(&self) -> Vec<InstalledRecord> {
        self.state()
            .plugins
            .into_iter()
            .map(|e| InstalledRecord {
                origin: match (e.source, e.sha256) {
                    (Some(source), Some(sha256)) => Some(Origin { source, sha256 }),
                    _ => None,
                },
                id: e.id,
                version: e.version,
                granted: e.granted,
                enabled: e.enabled,
            })
            .collect()
    }

    /// The installed module `id`, unchanged.
    pub fn module_bytes(&self, id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.module_path(id)).ok()
    }

    /// A small file next to the plugins (`sources.json`), or `None`.
    pub fn read_aux(&self, name: &str) -> Option<Vec<u8>> {
        if !aux_name(name) {
            return None;
        }
        std::fs::read(self.dir.join(name)).ok()
    }

    /// Writes a small file next to the plugins (atomically).
    pub fn write_aux(&self, name: &str, bytes: &[u8]) -> Result<(), PluginError> {
        if !aux_name(name) {
            return Err(io(format!("{name:?} is not a file name of the plugin folder")));
        }
        write_atomic(&self.dir.join(name), bytes).map_err(io)
    }

    /// Removes a plugin, its module and its storage.
    pub fn remove(&self, id: &str) -> Result<(), PluginError> {
        let mut state = self.state();
        let before = state.plugins.len();
        state.plugins.retain(|e| e.id != id);
        if state.plugins.len() == before {
            return Err(PluginError::NotFound(id.to_owned()));
        }
        self.save_state(&state)?;
        for path in [self.module_path(id), self.storage_path(id)] {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), PluginError> {
        let mut state = self.state();
        let entry = state
            .plugins
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        entry.enabled = enabled;
        self.save_state(&state)
    }

    fn storage_path(&self, id: &str) -> PathBuf {
        self.dir.join("storage").join(format!("{id}.json"))
    }

    /// The plugins' `storage`, in this folder.
    pub fn storage(&self) -> FileStorage {
        FileStorage { dir: self.clone() }
    }
}

/// `storage` as one JSON object per plugin in `storage/<id>.json`.
#[derive(Debug, Clone)]
pub struct FileStorage {
    dir: PluginDir,
}

impl FileStorage {
    fn read(&self, plugin: &str) -> serde_json::Map<String, Value> {
        std::fs::read(self.dir.storage_path(plugin))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
}

impl PluginStorage for FileStorage {
    fn get<'a>(&'a self, plugin: &'a str, key: &'a str) -> LocalBoxFuture<'a, Option<Value>> {
        let value = self.read(plugin).get(key).cloned();
        Box::pin(async move { value })
    }

    fn set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<Value>,
    ) -> LocalBoxFuture<'a, ()> {
        let mut map = self.read(plugin);
        match value {
            Some(v) => map.insert(key.to_owned(), v),
            None => map.remove(key),
        };
        if let Ok(json) = serde_json::to_vec(&map)
            && let Err(e) = write_atomic(&self.dir.storage_path(plugin), &json)
        {
            eprintln!("RequestTailor: cannot write the storage of plugin {plugin}: {e}");
        }
        Box::pin(async {})
    }
}
