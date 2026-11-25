// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The catalogue's device state in the plugin folder (desktop and CLI): the sources the user
//! added (`plugins/sources.json`) and, per installed plugin, where it came from
//! (`plugins/plugins.json`, see `rt_plugin::host::dir`). The web shell keeps the same data in
//! browser storage.

use rt_plugin::host::dir::{Origin, PluginDir};

use super::index::locate;
use super::plan::InstalledState;
use super::{CatalogueSource, InstalledSource};
use crate::AppError;

const SOURCES_FILE: &str = "sources.json";

/// A view of the plugin folder for the catalogue.
#[derive(Debug, Clone)]
pub struct DiskCatalogue {
    dir: PluginDir,
}

impl DiskCatalogue {
    pub fn new(dir: PluginDir) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &PluginDir {
        &self.dir
    }

    /// The sources, in the order they were added.
    pub fn sources(&self) -> Vec<CatalogueSource> {
        self.dir
            .read_aux(SOURCES_FILE)
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, sources: &[CatalogueSource]) -> Result<(), AppError> {
        let json = serde_json::to_vec_pretty(sources).map_err(|e| AppError::Shell(e.to_string()))?;
        Ok(self.dir.write_aux(SOURCES_FILE, &json)?)
    }

    /// Adds a source (checked, not requested). Adding one twice keeps the first.
    pub fn add_source(&self, url: &str) -> Result<CatalogueSource, AppError> {
        locate(url)?;
        let url = url.trim().to_owned();
        let mut all = self.sources();
        if let Some(existing) = all.iter().find(|s| s.url == url) {
            return Ok(existing.clone());
        }
        let source = CatalogueSource {
            url,
            commit: None,
            name: None,
        };
        all.push(source.clone());
        self.save(&all)?;
        Ok(source)
    }

    pub fn remove_source(&self, url: &str) -> Result<(), AppError> {
        let url = url.trim();
        let mut all = self.sources();
        let before = all.len();
        all.retain(|s| s.url != url);
        if all.len() == before {
            return Err(AppError::Plugin(format!("{url} is not one of your sources")));
        }
        self.save(&all)
    }

    /// Remembers what a refresh learned (the index's name, the commit).
    pub fn remember(&self, refreshed: &CatalogueSource) -> Result<(), AppError> {
        let mut all = self.sources();
        if let Some(s) = all.iter_mut().find(|s| s.url == refreshed.url)
            && s != refreshed
        {
            *s = refreshed.clone();
            self.save(&all)?;
        }
        Ok(())
    }

    /// The plugins installed from a source.
    pub fn installed_sources(&self) -> Vec<InstalledSource> {
        self.dir
            .records()
            .into_iter()
            .filter_map(|r| {
                let o = r.origin?;
                Some(InstalledSource {
                    plugin: r.id,
                    source: o.source,
                    version: r.version,
                    sha256: o.sha256,
                })
            })
            .collect()
    }

    /// What the plan needs to know about the installed plugin `id`.
    pub fn installed_state(&self, id: &str) -> Option<InstalledState> {
        self.dir
            .records()
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| InstalledState {
                version: r.version,
                granted: r.granted,
                sha256: r.origin.map(|o| o.sha256),
            })
    }

    /// Installs a verified module and records its origin.
    pub fn install(&self, wasm: &[u8], source: &str, sha256: &str) -> Result<(), AppError> {
        self.dir.install_from(
            wasm,
            Some(Origin {
                source: source.to_owned(),
                sha256: sha256.to_owned(),
            }),
        )?;
        Ok(())
    }
}
