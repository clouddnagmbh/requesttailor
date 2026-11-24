// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plugins for `rt send` (wave 5, strand A): the installed plugins (the directory the desktop
//! app installs into, `rt_plugin::host::dir`; `RT_DATA_DIR` overrides the data folder) plus
//! `--plugin` files, run by `rt_plugin::host::PluginHost` exactly as in the apps.
//!
//! - Installed plugins run in installation order, then the `--plugin` files in argument order.
//!   A `--plugin` file with the id of an installed plugin replaces it for this run.
//! - `--plugin` files get the permissions their manifest requests: naming the file is the
//!   user's consent. Installed plugins get what was granted at install time.
//! - `--no-plugins` skips the installed plugins (the `--plugin` files still run).
//! - An installed plugin that does not load is reported on stderr and skipped; a `--plugin`
//!   file that does not load fails the send.
//! - Plugin log lines (`log` host call, stdout and stderr of the module) go to stderr.
//! - `http` with `session: true` (wave 6) uses the `--cookie-jar` jar of the send; without
//!   one it is `credentials: true`.

use std::path::Path;
use std::rc::Rc;

use rt_core::services::{
    AfterResponse, HookContext, LocalBoxFuture, Note, NoteLevel, RequestHooks,
};
use rt_core::transport::native::NativeTransport;
use rt_core::transport::{HttpRequest, ResponseHead};
use rt_plugin::host::dir::{FileStorage, PluginDir};
use rt_plugin::host::services::{MemoryStorage, PluginStorage, TransportServices};
use rt_plugin::host::{Plugin, PluginHost, SessionHooks};
use serde_json::Value;

use crate::SendArgs;
use crate::cookies::CliCookies;

pub struct CliPlugins {
    host: PluginHost,
    cookies: Option<Rc<CliCookies>>,
}

/// Storage of the shared plugin folder, or memory when there is no data folder.
enum CliStorage {
    File(FileStorage),
    Memory(MemoryStorage),
}

impl PluginStorage for CliStorage {
    fn get<'a>(&'a self, plugin: &'a str, key: &'a str) -> LocalBoxFuture<'a, Option<Value>> {
        match self {
            Self::File(s) => s.get(plugin, key),
            Self::Memory(s) => s.get(plugin, key),
        }
    }

    fn set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<Value>,
    ) -> LocalBoxFuture<'a, ()> {
        match self {
            Self::File(s) => s.set(plugin, key, value),
            Self::Memory(s) => s.set(plugin, key, value),
        }
    }
}

fn level(level: NoteLevel) -> &'static str {
    match level {
        NoteLevel::Info => "info",
        NoteLevel::Warning => "warning",
        NoteLevel::Error => "error",
    }
}

impl CliPlugins {
    /// `None` when there is nothing to run.
    /// `cookies`: the send's cookie jar (`--cookie-jar`), shared with plugin `http` calls in
    /// the session (wave 6).
    pub fn new(
        args: &SendArgs,
        root: &Path,
        cookies: Option<Rc<CliCookies>>,
    ) -> Result<Option<Self>, String> {
        // Plugins come from the data folder and the arguments, not from the collection.
        let _ = root;
        let dir = PluginDir::shared();
        let mut plugins: Vec<Plugin> = Vec::new();
        if !args.no_plugins
            && let Some(dir) = &dir
        {
            let (loaded, failed) = dir.load();
            for (id, e) in failed {
                eprintln!("rt: installed plugin {id} not loaded: {e}");
            }
            plugins.extend(loaded);
        }
        for path in &args.plugins {
            let wasm = std::fs::read(path)
                .map_err(|e| format!("cannot read plugin {}: {e}", path.display()))?;
            let manifest =
                PluginHost::inspect(&wasm).map_err(|e| format!("{}: {e}", path.display()))?;
            let plugin = Plugin::compile(&wasm, &manifest.permissions, true)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            plugins.retain(|p| p.id() != plugin.id());
            plugins.push(plugin);
        }
        if plugins.is_empty() {
            return Ok(None);
        }
        let storage = match &dir {
            Some(dir) => CliStorage::File(dir.storage()),
            None => CliStorage::Memory(MemoryStorage::default()),
        };
        let services =
            TransportServices::new(NativeTransport::new().ok(), storage).with_logger(Box::new(
                |plugin, l, message| eprintln!("rt: plugin {plugin} ({}): {message}", level(l)),
            ));
        let host = PluginHost::with_plugins(Box::new(services), plugins);
        if !host.has_hooks() {
            return Ok(None);
        }
        Ok(Some(Self { host, cookies }))
    }

    pub fn hooks(&self) -> &dyn RequestHooks {
        self
    }

    fn session(&self) -> SessionHooks<'_> {
        self.host
            .in_session(self.cookies.as_deref().map(CliCookies::jar))
    }
}

impl RequestHooks for CliPlugins {
    fn before_send<'a>(
        &'a self,
        request: HttpRequest,
        secrets: &'a [String],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Result<(HttpRequest, Vec<Note>), String>> {
        Box::pin(async move { self.session().before_send(request, secrets, context).await })
    }

    fn after_response<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Vec<Note>> {
        Box::pin(async move { self.session().after_response(head, body, context).await })
    }

    fn after_response_full<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, AfterResponse> {
        Box::pin(async move {
            self.session()
                .after_response_full(head, body, context)
                .await
        })
    }
}
