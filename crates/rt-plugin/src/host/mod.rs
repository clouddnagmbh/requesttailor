// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin host (wave 5, strand A): loads plugin modules, checks their manifest, runs
//! their hooks under fuel and memory limits, and answers their host calls within the granted
//! permissions. Implements `rt_core::services::RequestHooks`, so `send_with` runs the enabled
//! plugins' `prepare` and `response` hooks on every target.
//!
//! - [`runtime`]: the WebAssembly interpreter (`wasmi`), the ABI of `API.md`, limits.
//! - [`permissions`]: what a host call may do; [`mask`]: secrets never reach a plugin.
//! - [`services`]: [`HostServices`] over any `rt_core` transport and a storage.
//! - [`dir`]: installed plugins in the data folder (desktop and CLI; not in the browser).
//!
//! Each call runs in a fresh instance of the module: memory and globals do not survive from
//! one call to the next (tab `state` and `storage` do).

mod convert;
#[cfg(not(target_arch = "wasm32"))]
pub mod dir;
pub mod manifest;
pub mod mask;
pub mod permissions;
pub mod runtime;
pub mod services;

use std::cell::RefCell;

use rt_core::services::{HookContext, LocalBoxFuture, Note, NoteLevel, RequestHooks};
use rt_core::transport::{HttpRequest, ResponseHead};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::api::{
    Call, DetectReply, HostCall, ImportReply, Manifest, NotesReply, Permissions, PluginContext,
    PluginFile, PluginNote, PluginRequest, PluginResponse, PrepareReply, Reply, TabPlace,
    TabTarget, ViewReply,
};
pub use convert::response_to_plugin;
use mask::Masker;
pub use runtime::Limits;

/// Bodies handed to plugins (responses of hooks, tabs and `http`) are cut at this size.
pub const PLUGIN_BODY_LIMIT: usize = 8 * 1024 * 1024;
/// Largest value a plugin may keep under one storage key (as JSON).
pub const STORAGE_VALUE_LIMIT: usize = 1024 * 1024;

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

impl LoadedPlugin {
    /// Whether the user granted everything the manifest requests.
    pub fn fully_granted(&self) -> bool {
        self.granted == self.manifest.permissions
    }
}

/// A compiled plugin: cheap to clone, `Send + Sync`, so shells keep them across sends and
/// threads and build a [`PluginHost`] per send.
#[derive(Debug, Clone)]
pub struct Plugin {
    pub info: LoadedPlugin,
    module: wasmi::Module,
}

impl Plugin {
    /// Checks the manifest and the module (validation, imports, exports) and compiles it.
    /// `granted` counts only if it covers what the manifest requests (all or nothing).
    pub fn compile(wasm: &[u8], granted: &Permissions, enabled: bool) -> Result<Self, PluginError> {
        let manifest = manifest::read(wasm)?;
        let module = runtime::compile(wasm).map_err(PluginError::Invalid)?;
        let granted = permissions::effective(granted, &manifest.permissions);
        Ok(Self {
            info: LoadedPlugin {
                manifest,
                granted,
                enabled,
            },
            module,
        })
    }

    pub fn id(&self) -> &str {
        &self.info.manifest.id
    }
}

/// The request of the send in progress, for the `response` hook, `{request}` and
/// `credentials`.
struct Current {
    request: HttpRequest,
    masker: Masker,
}

/// The plugins of one shell session (or one send).
pub struct PluginHost {
    services: Box<dyn HostServices>,
    plugins: Vec<LoadedPlugin>,
    modules: Vec<wasmi::Module>,
    limits: Limits,
    current: RefCell<Option<Current>>,
}

impl PluginHost {
    pub fn new(services: Box<dyn HostServices>) -> Self {
        Self::with_plugins(services, Vec::new())
    }

    /// A host with already compiled plugins, in installation order.
    pub fn with_plugins(services: Box<dyn HostServices>, plugins: Vec<Plugin>) -> Self {
        let mut host = Self {
            services,
            plugins: Vec::new(),
            modules: Vec::new(),
            limits: Limits::default(),
            current: RefCell::new(None),
        };
        for p in plugins {
            host.insert(p);
        }
        host
    }

    /// Fuel and memory per call (tests use small limits).
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// Reads and checks the manifest of a plugin module without running it. The module is
    /// validated too, so what passes here installs.
    pub fn inspect(wasm: &[u8]) -> Result<Manifest, PluginError> {
        let manifest = manifest::read(wasm)?;
        runtime::compile(wasm).map_err(PluginError::Invalid)?;
        Ok(manifest)
    }

    /// Compiles and instantiates a plugin. Replaces a loaded plugin with the same id.
    pub fn load(
        &mut self,
        wasm: &[u8],
        granted: Permissions,
        enabled: bool,
    ) -> Result<&LoadedPlugin, PluginError> {
        let plugin = Plugin::compile(wasm, &granted, enabled)?;
        Ok(self.insert(plugin))
    }

    /// Adds a compiled plugin; one with the same id is replaced in place (an update keeps
    /// its position in the order).
    pub fn insert(&mut self, plugin: Plugin) -> &LoadedPlugin {
        let i = match self.position(plugin.id()) {
            Some(i) => {
                self.plugins[i] = plugin.info;
                self.modules[i] = plugin.module;
                i
            }
            None => {
                self.plugins.push(plugin.info);
                self.modules.push(plugin.module);
                self.plugins.len() - 1
            }
        };
        &self.plugins[i]
    }

    pub fn unload(&mut self, id: &str) {
        if let Some(i) = self.position(id) {
            self.plugins.remove(i);
            self.modules.remove(i);
        }
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

    /// The compiled plugins, to build another host from (e.g. one per send).
    pub fn compiled(&self) -> Vec<Plugin> {
        self.plugins
            .iter()
            .zip(&self.modules)
            .map(|(info, module)| Plugin {
                info: info.clone(),
                module: module.clone(),
            })
            .collect()
    }

    /// Whether any enabled plugin has a `prepare` or `response` hook.
    pub fn has_hooks(&self) -> bool {
        self.plugins.iter().any(|p| {
            p.enabled && (p.manifest.contributes.prepare || p.manifest.contributes.response)
        })
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.plugins.iter().position(|p| p.manifest.id == id)
    }

    /// The enabled plugin `id`.
    fn enabled(&self, id: &str) -> Result<usize, PluginError> {
        let i = self
            .position(id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        if !self.plugins[i].enabled {
            return Err(PluginError::Refused(format!("plugin {id} is disabled")));
        }
        Ok(i)
    }

    /// Runs one call of plugin `i` and parses its `Reply`.
    async fn run(
        &self,
        i: usize,
        call: &Call,
        request: Option<&HttpRequest>,
        masker: &Masker,
    ) -> Result<Reply<Value>, PluginError> {
        let plugin = &self.plugins[i];
        let id = &plugin.manifest.id;
        let failed = |message: String| PluginError::Failed {
            id: id.clone(),
            message,
        };
        let input = serde_json::to_vec(call).map_err(|e| failed(e.to_string()))?;
        let scope = Scope {
            host: self,
            plugin,
            request,
            masker,
        };
        let outcome = runtime::call(&self.modules[i], self.limits, &input, &scope)
            .await
            .map_err(failed)?;
        for line in String::from_utf8_lossy(&outcome.log).lines() {
            if !line.trim().is_empty() {
                self.services.log(id, NoteLevel::Info, line);
            }
        }
        let reply: Reply<Value> = serde_json::from_slice(&outcome.reply)
            .map_err(|e| failed(format!("invalid reply: {e}")))?;
        Ok(reply)
    }

    /// `ok` of a reply as `T`; `None` or `null` is `T::default()` (hooks without changes).
    fn decode<T: DeserializeOwned + Default>(
        &self,
        i: usize,
        ok: Option<Value>,
    ) -> Result<T, PluginError> {
        match ok {
            None | Some(Value::Null) => Ok(T::default()),
            Some(v) => self.decode_required(i, Some(v)),
        }
    }

    fn decode_required<T: DeserializeOwned>(
        &self,
        i: usize,
        ok: Option<Value>,
    ) -> Result<T, PluginError> {
        let failed = |message: String| PluginError::Failed {
            id: self.plugins[i].manifest.id.clone(),
            message,
        };
        let v = ok.ok_or_else(|| failed("the reply has neither ok nor error".into()))?;
        serde_json::from_value(v).map_err(|e| failed(format!("invalid reply: {e}")))
    }

    fn context(&self, i: usize, context: &HookContext, masker: &Masker) -> PluginContext {
        PluginContext {
            request_path: context.request_path.clone(),
            environment: context.environment.clone(),
            variables: if self.plugins[i].granted.variables {
                context
                    .variables
                    .iter()
                    .map(|(n, v)| (n.clone(), masker.text(v)))
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    fn notes(&self, i: usize, notes: Vec<PluginNote>) -> impl Iterator<Item = Note> + use<> {
        let source = format!("plugin:{}", self.plugins[i].manifest.id);
        notes.into_iter().map(move |n| Note {
            source: source.clone(),
            level: n.level,
            text: n.text,
        })
    }

    async fn prepare(
        &self,
        mut request: HttpRequest,
        secrets: &[String],
        context: &HookContext,
    ) -> Result<(HttpRequest, Vec<Note>), String> {
        let masker = Masker::new(secrets);
        let mut notes = Vec::new();
        for i in 0..self.plugins.len() {
            let p = &self.plugins[i];
            if !p.enabled || !p.manifest.contributes.prepare {
                continue;
            }
            let id = &p.manifest.id;
            let call = Call::Prepare {
                request: convert::request_to_plugin(&request, &masker),
                context: self.context(i, context, &masker),
            };
            let reply = self
                .run(i, &call, Some(&request), &masker)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(error) = reply.error {
                return Err(format!("plugin {id}: {error}"));
            }
            let reply: PrepareReply = self.decode(i, reply.ok).map_err(|e| e.to_string())?;
            convert::apply_patch(&mut request, &reply.patch)
                .map_err(|e| format!("plugin {id}: {e}"))?;
            notes.extend(self.notes(i, reply.notes));
        }
        *self.current.borrow_mut() = Some(Current {
            request: request.clone(),
            masker,
        });
        Ok((request, notes))
    }

    async fn response(&self, head: &ResponseHead, body: &[u8], context: &HookContext) -> Vec<Note> {
        let (request, masker) = match self.current.borrow().as_ref() {
            Some(c) => (Some(c.request.clone()), c.masker.clone()),
            None => (None, Masker::default()),
        };
        let mut notes = Vec::new();
        for i in 0..self.plugins.len() {
            let p = &self.plugins[i];
            if !p.enabled || !p.manifest.contributes.response {
                continue;
            }
            let call = Call::Response {
                request: match &request {
                    Some(r) => convert::request_to_plugin(r, &masker),
                    None => PluginRequest {
                        method: "GET".into(),
                        url: masker.text(&head.url),
                        headers: Vec::new(),
                        body: None,
                    },
                },
                response: response_to_plugin(head, body, false, PLUGIN_BODY_LIMIT, &masker),
                context: self.context(i, context, &masker),
            };
            let source = format!("plugin:{}", p.manifest.id);
            let error = |text: String| Note {
                source: source.clone(),
                level: NoteLevel::Error,
                text,
            };
            match self.run(i, &call, request.as_ref(), &masker).await {
                Ok(Reply {
                    error: Some(message),
                    ..
                }) => notes.push(error(message)),
                Ok(reply) => match self.decode::<NotesReply>(i, reply.ok) {
                    Ok(r) => notes.extend(self.notes(i, r.notes)),
                    Err(e) => notes.push(error(e.to_string())),
                },
                Err(e) => notes.push(error(e.to_string())),
            }
        }
        notes
    }

    /// Renders a tab, or runs one of its actions (`action: Some`).
    #[allow(clippy::too_many_arguments)]
    pub async fn tab(
        &self,
        plugin: &str,
        tab: &str,
        action: Option<(&str, Vec<(String, String)>)>,
        target: TabTarget,
        spec: rt_core::model::RequestSpec,
        response: Option<PluginResponse>,
        state: Option<serde_json::Value>,
    ) -> Result<ViewReply, PluginError> {
        let i = self.enabled(plugin)?;
        let place = self.plugins[i]
            .manifest
            .contributes
            .tabs
            .iter()
            .find(|t| t.id == tab)
            .map(|t| t.place)
            .ok_or_else(|| PluginError::NotFound(format!("{plugin}/{tab}")))?;
        let call = match action {
            // `context` (wave 6, strand F): the shells pass none yet.
            None => Call::RenderTab {
                tab: tab.to_owned(),
                target,
                spec,
                response,
                state,
                context: None,
            },
            Some((action, input)) => Call::Action {
                tab: tab.to_owned(),
                action: action.to_owned(),
                target,
                spec,
                response,
                state,
                input,
                context: None,
            },
        };
        let reply = self.run(i, &call, None, &Masker::default()).await?;
        if let Some(error) = reply.error {
            return Err(PluginError::Refused(error));
        }
        let mut view: ViewReply = self.decode_required(i, reply.ok)?;
        if place == TabPlace::Response {
            view.update_request = None;
        }
        Ok(view)
    }

    fn importer(&self, plugin: &str, importer: &str) -> Result<usize, PluginError> {
        let i = self.enabled(plugin)?;
        if !self.plugins[i]
            .manifest
            .contributes
            .importers
            .iter()
            .any(|im| im.id == importer)
        {
            return Err(PluginError::NotFound(format!("{plugin}/{importer}")));
        }
        Ok(i)
    }

    pub async fn detect(
        &self,
        plugin: &str,
        importer: &str,
        files: Vec<PluginFile>,
    ) -> Result<bool, PluginError> {
        let i = self.importer(plugin, importer)?;
        let call = Call::Detect {
            importer: importer.to_owned(),
            files,
        };
        let reply = self.run(i, &call, None, &Masker::default()).await?;
        if let Some(error) = reply.error {
            return Err(PluginError::Refused(error));
        }
        let reply: DetectReply = self.decode_required(i, reply.ok)?;
        Ok(reply.matches)
    }

    pub async fn import(
        &self,
        plugin: &str,
        importer: &str,
        files: Vec<PluginFile>,
    ) -> Result<ImportReply, PluginError> {
        let i = self.importer(plugin, importer)?;
        let call = Call::Import {
            importer: importer.to_owned(),
            files,
        };
        let reply = self.run(i, &call, None, &Masker::default()).await?;
        if let Some(error) = reply.error {
            return Err(PluginError::Refused(error));
        }
        self.decode_required(i, reply.ok)
    }
}

impl RequestHooks for PluginHost {
    fn before_send<'a>(
        &'a self,
        request: HttpRequest,
        secrets: &'a [String],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Result<(HttpRequest, Vec<Note>), String>> {
        Box::pin(self.prepare(request, secrets, context))
    }

    fn after_response<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Vec<Note>> {
        Box::pin(self.response(head, body, context))
    }
}

/// Answers the host calls of one call of one plugin.
struct Scope<'a> {
    host: &'a PluginHost,
    plugin: &'a LoadedPlugin,
    /// The real request of the hook (`prepare`, `response`), else `None`.
    request: Option<&'a HttpRequest>,
    masker: &'a Masker,
}

fn ok(value: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({ "ok": value })).expect("JSON")
}

fn error(message: impl Into<String>) -> Vec<u8> {
    serde_json::to_vec(&json!({ "error": message.into() })).expect("JSON")
}

impl Scope<'_> {
    async fn answer(&self, raw: Vec<u8>) -> Vec<u8> {
        let call: HostCall = match serde_json::from_slice(&raw) {
            Ok(c) => c,
            Err(e) => return error(format!("invalid host call: {e}")),
        };
        let id = self.plugin.manifest.id.as_str();
        let services = &self.host.services;
        let granted = &self.plugin.granted;
        match call {
            HostCall::Log { level, message } => {
                services.log(id, level, &self.masker.text(&message));
                ok(Value::Null)
            }
            HostCall::Now => {
                let ms = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                ok(json!(ms))
            }
            // `session` (wave 6, strand F): the cookie jar is not wired yet; until then the
            // call runs without the session's cookies.
            HostCall::Http {
                request,
                credentials,
                session: _,
            } => match self.http(&request, credentials).await {
                Ok(response) => ok(serde_json::to_value(response).expect("JSON")),
                Err(e) => error(self.masker.text(&e)),
            },
            HostCall::StorageGet { key } => {
                if !granted.storage {
                    return error("permission denied: storage");
                }
                if let Err(e) = check_key(&key) {
                    return error(e);
                }
                ok(services.storage_get(id, &key).await.unwrap_or(Value::Null))
            }
            HostCall::StorageSet { key, value } => {
                if !granted.storage {
                    return error("permission denied: storage");
                }
                if let Err(e) = check_key(&key) {
                    return error(e);
                }
                if serde_json::to_vec(&value).map_or(0, |v| v.len()) > STORAGE_VALUE_LIMIT {
                    return error("storage value too large");
                }
                services.storage_set(id, &key, Some(value)).await;
                ok(Value::Null)
            }
            HostCall::StorageDelete { key } => {
                if !granted.storage {
                    return error("permission denied: storage");
                }
                if let Err(e) = check_key(&key) {
                    return error(e);
                }
                services.storage_set(id, &key, None).await;
                ok(Value::Null)
            }
        }
    }

    async fn http(
        &self,
        request: &PluginRequest,
        credentials: bool,
    ) -> Result<PluginResponse, String> {
        let mut outgoing = convert::request_from_plugin(request)?;
        let hook_url = self.request.map(|r| r.url.as_str());
        if !permissions::network_allows(&self.plugin.granted.network, &outgoing.url, hook_url) {
            return Err("permission denied: network".into());
        }
        if credentials {
            let current = self
                .request
                .ok_or("credentials are only available in the prepare and response hooks")?;
            if !permissions::same_origin(&current.url, &outgoing.url) {
                return Err("credentials are only sent to the origin of the request".into());
            }
            for (name, value) in &current.headers {
                if self.masker.hits(value) {
                    outgoing
                        .headers
                        .retain(|(n, _)| !n.eq_ignore_ascii_case(name));
                    outgoing.headers.push((name.clone(), value.clone()));
                }
            }
        }
        let (head, body, truncated) = self.host.services.http(outgoing).await?;
        Ok(response_to_plugin(
            &head,
            &body,
            truncated,
            PLUGIN_BODY_LIMIT,
            self.masker,
        ))
    }
}

fn check_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() || key.len() > 256 {
        return Err("storage keys have 1 to 256 bytes");
    }
    Ok(())
}

impl runtime::HostCalls for Scope<'_> {
    fn host_call<'a>(&'a self, call: Vec<u8>) -> LocalBoxFuture<'a, Vec<u8>> {
        Box::pin(self.answer(call))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn compiled_plugins_move_between_threads() {
        assert_send_sync::<Plugin>();
    }
}
