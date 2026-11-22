// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin host (wave 5, strand A): loads plugin modules, checks their manifest, runs
//! their hooks under fuel and memory limits, and answers their host calls within the granted
//! permissions. Implements `rt_core::services::RequestHooks`, so `send_with` runs the enabled
//! plugins' `prepare` and `response` hooks on every target.
//!
//! Interface commit: the shape the shells and `rt-app` program against; every operation
//! fails with `PluginError::Unavailable`. Strand A owns this file (and may split it).

use rt_core::services::{LocalBoxFuture, NoteLevel};
use rt_core::transport::{HttpRequest, ResponseHead};

use crate::api::{ImportReply, Manifest, Permissions, PluginFile, PluginResponse, ViewReply};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginError {
    /// Not a plugin: not WebAssembly, no or invalid manifest section, bad id.
    #[error("not a RequestTailor plugin: {0}")]
    Invalid(String),
    /// Built for an API version this host does not run.
    #[error("plugin needs API version {needs}; this RequestTailor runs 1 to {runs}")]
    ApiVersion { needs: u32, runs: u32 },
    #[error("plugin {0} is not installed")]
    NotFound(String),
    /// The plugin trapped, ran out of fuel or memory, or answered garbage.
    #[error("plugin {id} failed: {message}")]
    Failed { id: String, message: String },
    /// The plugin returned `error`.
    #[error("{0}")]
    Refused(String),
    #[error("plugins are not available yet")]
    Unavailable,
}

/// A response for a plugin: head, body (cut at the host's limit), and whether it was cut.
pub type HostResponse = (ResponseHead, Vec<u8>, bool);

/// What the host needs from the shell to answer host calls.
pub trait HostServices {
    /// Sends a request for a plugin (permission already checked) through the shell's
    /// transport. The response body is read up to the host's limit.
    fn http<'a>(&'a self, request: HttpRequest)
    -> LocalBoxFuture<'a, Result<HostResponse, String>>;
    fn storage_get<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
    ) -> LocalBoxFuture<'a, Option<serde_json::Value>>;
    fn storage_set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<serde_json::Value>,
    ) -> LocalBoxFuture<'a, ()>;
    fn log(&self, plugin: &str, level: NoteLevel, message: &str);
}

/// One installed plugin as the host knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedPlugin {
    pub manifest: Manifest,
    /// What the user granted: equal to `manifest.permissions` or nothing (API version 1).
    pub granted: Permissions,
    pub enabled: bool,
}

/// The plugins of one shell session.
pub struct PluginHost {
    services: Box<dyn HostServices>,
    plugins: Vec<LoadedPlugin>,
}

impl PluginHost {
    pub fn new(services: Box<dyn HostServices>) -> Self {
        Self {
            services,
            plugins: Vec::new(),
        }
    }

    /// Reads and checks the manifest of a plugin module without running it.
    pub fn inspect(wasm: &[u8]) -> Result<Manifest, PluginError> {
        let _ = wasm;
        Err(PluginError::Unavailable)
    }

    /// Compiles and instantiates a plugin. Replaces a loaded plugin with the same id.
    pub fn load(
        &mut self,
        wasm: &[u8],
        granted: Permissions,
        enabled: bool,
    ) -> Result<&LoadedPlugin, PluginError> {
        let _ = (wasm, granted, enabled, &self.services);
        Err(PluginError::Unavailable)
    }

    pub fn unload(&mut self, id: &str) {
        self.plugins.retain(|p| p.manifest.id != id);
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), PluginError> {
        let plugin = self
            .plugins
            .iter_mut()
            .find(|p| p.manifest.id == id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        plugin.enabled = enabled;
        Ok(())
    }

    pub fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    /// Renders a tab, or runs one of its actions (`action: Some`).
    #[allow(clippy::too_many_arguments)]
    pub async fn tab(
        &self,
        plugin: &str,
        tab: &str,
        action: Option<(&str, Vec<(String, String)>)>,
        target: crate::api::TabTarget,
        spec: rt_core::model::RequestSpec,
        response: Option<PluginResponse>,
        state: Option<serde_json::Value>,
    ) -> Result<ViewReply, PluginError> {
        let _ = (plugin, tab, action, target, spec, response, state);
        Err(PluginError::Unavailable)
    }

    pub async fn detect(
        &self,
        plugin: &str,
        importer: &str,
        files: Vec<PluginFile>,
    ) -> Result<bool, PluginError> {
        let _ = (plugin, importer, files);
        Err(PluginError::Unavailable)
    }

    pub async fn import(
        &self,
        plugin: &str,
        importer: &str,
        files: Vec<PluginFile>,
    ) -> Result<ImportReply, PluginError> {
        let _ = (plugin, importer, files);
        Err(PluginError::Unavailable)
    }
}
