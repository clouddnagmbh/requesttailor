// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin catalogue without a server (wave 18, strand I). A source is a URL the user types
//! in (an HTTPS link to a `plugins.json` or a Git repository that has one); there is no built-in
//! index and nothing is requested before the user presses "Refresh" or runs `rt plugin`.
//! Before installing: the SHA-256 of the downloaded bytes must equal the index's; the
//! permissions shown come from the manifest inside the `.wasm`, not from the index (a
//! difference warns); an update with the same version but another checksum is refused; more
//! permissions ask again. Only WebAssembly modules are installed
//! (`rt_plugin::host::dir::PluginDir::install`). Interface: types only.

use serde::{Deserialize, Serialize};

pub use rt_plugin::api::Permissions;

pub const INDEX_VERSION: u32 = 1;
/// At most this many entries in an index.
pub const MAX_ENTRIES: usize = 1_000;
/// The index file is at most this large (1 MiB).
pub const MAX_INDEX_BYTES: usize = 1024 * 1024;
/// A plugin module is at most this large (32 MiB).
pub const MAX_WASM_BYTES: usize = 32 * 1024 * 1024;

/// `plugins.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueIndex {
    pub version: u32,
    pub name: String,
    pub plugins: Vec<CatalogueEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    pub wasm: CatalogueWasm,
    /// What the index claims; the manifest in the module decides.
    #[serde(default)]
    pub permissions: Permissions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueWasm {
    /// Absolute, or relative to the index.
    pub url: String,
    /// Lower-case hex SHA-256 of the module.
    pub sha256: String,
    pub size: u64,
}

/// A source the user added (kept per device in `plugins/sources.json`, in the browser in its
/// storage).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueSource {
    /// The URL the user typed.
    pub url: String,
    /// The commit a Git source was read at, for "index at 3fa1c9e".
    #[serde(default)]
    pub commit: Option<String>,
    /// Name from the index, once it was refreshed.
    #[serde(default)]
    pub name: Option<String>,
}

/// A refreshed source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIndex {
    pub source: CatalogueSource,
    pub index: CatalogueIndex,
}

/// What installing an entry would do, checked but not done (`CatalogueApi.plan`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub source: String,
    pub entry: CatalogueEntry,
    /// SHA-256 of the downloaded module (equal to `entry.wasm.sha256`, else the plan is an
    /// error and is never returned).
    pub sha256: String,
    /// The rights the module's own manifest asks for.
    pub permissions: Permissions,
    /// The index named other rights than the manifest.
    pub permissions_differ: bool,
    /// An installed plugin with this id is replaced.
    pub update: bool,
    /// An update that asks for more rights than the installed version.
    pub more_permissions: bool,
    pub warnings: Vec<String>,
}

/// Where an installed plugin came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledSource {
    pub plugin: String,
    pub source: String,
    pub version: String,
    pub sha256: String,
}

/// A newer version in a source (`CatalogueApi.checkUpdates`, only when asked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueUpdate {
    pub plugin: String,
    pub source: String,
    pub installed: String,
    pub available: String,
}
