// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plugins in the desktop app (wave 5, strand A): installed modules live in the data folder's
//! `plugins/` directory (shared with the CLI, see `rt_plugin::host::dir`), a
//! `rt_plugin::host::PluginHost` runs them, and `Core::send` passes it to `send_with` as
//! `RequestHooks`. Commands: `plugin_list`, `plugin_inspect`, `plugin_install` (raw body: the
//! `.wasm` bytes), `plugin_remove`, `plugin_set_enabled`, `plugin_render_tab`,
//! `plugin_action`, `import_analyze_plugin` (raw body as `import_analyze`, header with
//! `plugin` and `importer`), `plugin_welcome`, `plugin_choose_welcome` (wave 6).
//!
//! The compiled plugins are kept in [`PluginParts`] (`Send + Sync`); a `PluginHost` is not
//! `Send`, so every send, tab call and import builds one on its blocking thread from them.
//!
//! Bundled plugins (wave 6, strand F): the app ships the plugins in [`BUNDLED`] as Tauri
//! resources (`bundled-plugins/<id>.wasm`, filled by `npm run bundle-plugins`; debug builds
//! also read `target/plugins`). The first `plugin_welcome` of a process installs them into
//! the plugin folder, disabled, or updates an older installed version (after the first frame,
//! so the cold start does not pay for it). `welcome.json` in the data folder records the
//! answer to the first-start question and which bundled plugins were installed
//! (`rt_app::plugins::WelcomeRecord`), so a bundled plugin the user removed stays removed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rt_app::import::SourceFile;
use rt_app::plugins::{
    BundledStep, Manifest, Plugin, PluginError, PluginHost, PluginInfo, Recording, ResponseHeads,
    TabTarget, ViewReply, WelcomeOffer, WelcomeRecord, WelcomeState, bundled_step, plugin_files,
    plugin_import,
};
use rt_app::{AppError, ImportPlan, SecretStore};
use rt_core::services::{
    AfterResponse, HookContext, LocalBoxFuture, Note, NoteLevel, RequestHooks,
};
use rt_core::transport::native::NativeTransport;
use rt_core::transport::{HttpRequest, ResponseHead};
use rt_format::import::MAX_INPUT_BYTES;
use rt_plugin::host::PLUGIN_BODY_LIMIT;
use rt_plugin::host::dir::{FileStorage, PluginDir};
use rt_plugin::host::services::{MemoryStorage, PluginStorage, TransportServices};
use serde::Deserialize;
use serde_json::Value;
use tauri::State;
use tauri::ipc::{InvokeBody, Request};

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

fn raw<'a>(request: &'a Request<'_>, command: &str) -> Result<&'a [u8], AppError> {
    match request.body() {
        InvokeBody::Raw(body) => Ok(body),
        _ => Err(AppError::Shell(format!("{command} expects raw bytes"))),
    }
}

#[tauri::command]
pub async fn plugin_list(core: State<'_, AppCore>) -> Result<Vec<PluginInfo>, CommandError> {
    Ok(core.services.plugins.list())
}

#[tauri::command]
pub async fn plugin_inspect(request: Request<'_>) -> Result<Manifest, CommandError> {
    Ok(PluginParts::inspect(raw(&request, "plugin_inspect")?)?)
}

#[tauri::command]
pub async fn plugin_install(
    core: State<'_, AppCore>,
    request: Request<'_>,
) -> Result<PluginInfo, CommandError> {
    Ok(core
        .services
        .plugins
        .install(raw(&request, "plugin_install")?)?)
}

#[tauri::command]
pub async fn plugin_remove(core: State<'_, AppCore>, id: String) -> Result<(), CommandError> {
    Ok(core.services.plugins.remove(&id)?)
}

#[tauri::command]
pub async fn plugin_set_enabled(
    core: State<'_, AppCore>,
    id: String,
    enabled: bool,
) -> Result<(), CommandError> {
    Ok(core.services.plugins.set_enabled(&id, enabled)?)
}

#[tauri::command]
pub async fn plugin_render_tab(
    core: State<'_, AppCore>,
    plugin: String,
    tab: String,
    target: TabTarget,
    state: Option<serde_json::Value>,
    environment: Option<String>,
) -> Result<ViewReply, CommandError> {
    Ok(render_tab(&core, plugin, tab, None, target, state, environment).await?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn plugin_action(
    core: State<'_, AppCore>,
    plugin: String,
    tab: String,
    action: String,
    target: TabTarget,
    state: Option<serde_json::Value>,
    input: Vec<(String, String)>,
    environment: Option<String>,
) -> Result<ViewReply, CommandError> {
    let action = Some((action, input));
    Ok(render_tab(&core, plugin, tab, action, target, state, environment).await?)
}

/// The first-start question (wave 6): bundled plugins with a `welcome` choice. Installs the
/// bundled plugins first (once per process, on a blocking thread).
#[tauri::command]
pub async fn plugin_welcome(core: State<'_, AppCore>) -> Result<WelcomeState, CommandError> {
    let parts = Arc::clone(&core.services);
    Ok(blocking(move || parts.plugins.welcome()).await?)
}

/// Enables the chosen bundled plugins and records that the question was answered.
#[tauri::command]
pub async fn plugin_choose_welcome(
    core: State<'_, AppCore>,
    plugins: Vec<String>,
) -> Result<(), CommandError> {
    let parts = Arc::clone(&core.services);
    Ok(blocking(move || parts.plugins.choose_welcome(&plugins)).await?)
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Shell(format!("plugin call failed: {e}")))?
}

#[tauri::command]
pub async fn import_analyze_plugin(
    core: State<'_, AppCore>,
    request: Request<'_>,
) -> Result<ImportPlan, CommandError> {
    Ok(analyze_with_plugin(&core, raw(&request, "import_analyze_plugin")?).await?)
}

/// Runs `f` with a fresh host on a blocking thread (a host and its futures are not `Send`).
async fn with_host<T, F>(parts: &Arc<crate::services::ServiceParts>, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: for<'h> FnOnce(&'h PluginHost) -> LocalBoxFuture<'h, Result<T, AppError>> + Send + 'static,
{
    let parts = Arc::clone(parts);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let host = parts.plugins.host();
        runtime.block_on(f(&host))
    })
    .await
    .map_err(|e| AppError::Shell(format!("plugin call failed: {e}")))?
}

/// Renders a plugin tab (`action: None`) or runs one of its actions, for the request as it is
/// in the editor (unsaved changes included) and the response in the response store. A
/// collection tab (`request_path` `""`) gets the default request. `environment` is the
/// selected one, for the plugin's `context` (wave 6).
pub async fn render_tab<S: SecretStore>(
    core: &Core<S>,
    plugin: String,
    tab: String,
    action: Option<(String, Vec<(String, String)>)>,
    target: TabTarget,
    state: Option<Value>,
    environment: Option<String>,
) -> Result<ViewReply, AppError> {
    let spec = if target.request_path.is_empty() {
        rt_core::model::RequestSpec::default()
    } else {
        core.get_request(&target.request_path)?
    };
    let scope = core.plugin_scope(&target.request_path, environment.as_deref())?;
    let collection = core.collection_id().unwrap_or_default();
    let response = match target.response_id {
        Some(id) => {
            let limit = u32::try_from(PLUGIN_BODY_LIMIT + 1).unwrap_or(u32::MAX);
            let body = core.response_bytes(id, 0, limit)?;
            Some(
                core.services
                    .plugins
                    .heads
                    .tab_response(&collection, &target.request_path, &body),
            )
        }
        None => None,
    };
    with_host(&core.services, move |host| {
        Box::pin(async move {
            let action = action
                .as_ref()
                .map(|(name, input)| (name.as_str(), input.clone()));
            Ok(host
                .tab_with(&plugin, &tab, action, target, spec, response, state, scope)
                .await?)
        })
    })
    .await
}

#[derive(Debug, Deserialize)]
struct ImportHeader {
    plugin: String,
    importer: String,
    files: Vec<ImportHeaderFile>,
}

#[derive(Debug, Deserialize)]
struct ImportHeaderFile {
    name: String,
    size: usize,
}

/// Named files of a packed plugin import.
type Files = Vec<(String, Vec<u8>)>;

/// Splits the body `packPluginImport` (`apps/desktop/src/import.ts`) builds.
fn unpack(body: &[u8]) -> Result<(ImportHeader, Files), AppError> {
    let bad = |why: &str| AppError::Shell(format!("malformed import request: {why}"));
    let len: [u8; 4] = body
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| bad("no header"))?;
    let header_len = u32::from_le_bytes(len) as usize;
    let header_bytes = body
        .get(4..4usize.saturating_add(header_len))
        .ok_or_else(|| bad("header is cut off"))?;
    let header: ImportHeader =
        serde_json::from_slice(header_bytes).map_err(|e| bad(&e.to_string()))?;
    let mut rest = &body[4 + header_len..];
    let mut files = Vec::with_capacity(header.files.len());
    for f in &header.files {
        if f.size > MAX_INPUT_BYTES || f.size > rest.len() {
            return Err(bad("file data is cut off"));
        }
        let (data, tail) = rest.split_at(f.size);
        files.push((f.name.clone(), data.to_vec()));
        rest = tail;
    }
    if !rest.is_empty() {
        return Err(bad("unexpected data after the files"));
    }
    Ok((header, files))
}

/// `ImportApi.analyzeWithPlugin`: the plugin's importer converts, the result is checked like a
/// built-in import and kept in `Core::imports` for `import_write`.
pub async fn analyze_with_plugin<S: SecretStore>(
    core: &Core<S>,
    body: &[u8],
) -> Result<ImportPlan, AppError> {
    let (header, files) = unpack(body)?;
    let sources: Vec<SourceFile<'_>> = files
        .iter()
        .map(|(name, data)| SourceFile { name, data })
        .collect();
    let input = plugin_files(&sources);
    let reply = with_host(&core.services, move |host| {
        Box::pin(async move {
            let (plugin, importer) = (header.plugin.as_str(), header.importer.as_str());
            if !host.detect(plugin, importer, input.clone()).await? {
                return Err(AppError::Import(format!(
                    "the importer of plugin {plugin} does not recognize these files"
                )));
            }
            Ok(host.import(plugin, importer, input).await?)
        })
    })
    .await?;
    let import = plugin_import(reply)?;
    let mut imports = core.imports.lock().unwrap_or_else(|p| p.into_inner());
    Ok(imports.add(import))
}

/// Storage of the plugin folder, or memory when the app persists nothing (tests).
enum DesktopStorage {
    File(FileStorage),
    Memory(MemoryStorage),
}

impl PluginStorage for DesktopStorage {
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

/// The plugins the app ships (Tauri resources `bundled-plugins/<id>.wasm`).
pub const BUNDLED: &[&str] = &["odata"];
/// The resource folder of the bundled plugins (`bundle.resources` in `tauri.conf.json`).
pub const BUNDLED_RESOURCES: &str = "bundled-plugins";
/// The first-start answer and the installed bundled plugins, in the data folder.
pub const WELCOME_FILE: &str = "welcome.json";

/// Installed plugins and their host, kept by `Core` (`services::ServiceParts`).
pub struct PluginParts {
    /// `None`: nothing is persisted (tests); installs live until the app quits.
    dir: Option<PluginDir>,
    /// Installation order.
    plugins: Mutex<Vec<Plugin>>,
    transport: Option<NativeTransport>,
    /// Response heads for response tabs.
    pub heads: ResponseHeads,
    /// Folders searched for bundled plugins, in order, and the ids of the bundled plugins
    /// (see [`PluginParts::bundle_from`]).
    bundled: Mutex<(Vec<PathBuf>, Vec<String>)>,
    /// `welcome.json`; `None` keeps the record in memory (tests).
    welcome_file: Option<PathBuf>,
    /// The record, read on first use; `bundled` once the bundled plugins were installed.
    welcome: Mutex<Welcome>,
}

#[derive(Default)]
struct Welcome {
    record: Option<WelcomeRecord>,
    bundled: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl PluginParts {
    pub fn new(data_dir: Option<&std::path::Path>) -> Self {
        let dir = data_dir.map(PluginDir::in_data_dir);
        let plugins = match &dir {
            Some(dir) => {
                let (loaded, failed) = dir.load();
                for (id, e) in failed {
                    eprintln!("RequestTailor: plugin {id} not loaded: {e}");
                }
                loaded
            }
            None => Vec::new(),
        };
        Self {
            dir,
            plugins: Mutex::new(plugins),
            transport: NativeTransport::new().ok(),
            heads: ResponseHeads::new(),
            bundled: Mutex::new((Vec::new(), Vec::new())),
            welcome_file: data_dir.map(|d| d.join(WELCOME_FILE)),
            welcome: Mutex::new(Welcome::default()),
        }
    }

    /// Where the bundled plugins `ids` ([`BUNDLED`]) are: `<dir>/<id>.wasm` in the first of
    /// `dirs` that has it ([`PluginParts::bundled_dirs`]). Set at start-up; nothing is read
    /// until the first [`PluginParts::welcome`].
    pub fn bundle_from(&self, dirs: Vec<PathBuf>, ids: &[&str]) {
        *lock(&self.bundled) = (dirs, ids.iter().map(|id| (*id).to_owned()).collect());
    }

    /// The folders for [`PluginParts::bundle_from`] given the app's resource folder.
    pub fn bundled_dirs(resource_dir: Option<&Path>) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = resource_dir
            .map(|d| d.join(BUNDLED_RESOURCES))
            .into_iter()
            .collect();
        if cfg!(debug_assertions) {
            dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/plugins"));
        }
        dirs
    }

    fn record<'a>(&self, welcome: &'a mut Welcome) -> &'a mut WelcomeRecord {
        welcome.record.get_or_insert_with(|| {
            self.welcome_file
                .as_deref()
                .and_then(|f| std::fs::read(f).ok())
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default()
        })
    }

    fn save_record(&self, record: &WelcomeRecord) -> Result<(), AppError> {
        let Some(path) = &self.welcome_file else {
            return Ok(());
        };
        let json = serde_json::to_vec_pretty(record).map_err(|e| AppError::Shell(e.to_string()))?;
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, &json)?;
            std::fs::rename(&tmp, path)
        };
        write().map_err(|e| AppError::Shell(format!("cannot save {WELCOME_FILE}: {e}")))
    }

    /// Installs the bundled plugins that are new (disabled) or newer than the installed
    /// version (keeping its enabled state). Once per process.
    fn ensure_bundled(&self, welcome: &mut Welcome) -> Result<(), AppError> {
        if welcome.bundled {
            return Ok(());
        }
        welcome.bundled = true;
        let (dirs, ids) = lock(&self.bundled).clone();
        let mut record = self.record(welcome).clone();
        for id in &ids {
            let Some(wasm) = dirs
                .iter()
                .find_map(|d| std::fs::read(d.join(format!("{id}.wasm"))).ok())
            else {
                continue;
            };
            let shipped = match rt_plugin::host::manifest::read(&wasm) {
                Ok(m) if m.id == **id => m,
                Ok(m) => {
                    eprintln!("RequestTailor: bundled {id}.wasm is plugin {}", m.id);
                    continue;
                }
                Err(e) => {
                    eprintln!("RequestTailor: bundled plugin {id}: {e}");
                    continue;
                }
            };
            let installed = lock(&self.plugins)
                .iter()
                .find(|p| p.id() == id.as_str())
                .map(|p| p.info.manifest.version.clone());
            let step = bundled_step(&record, installed.as_deref(), &shipped);
            let done = match step {
                BundledStep::Install => self.install_disabled(&wasm).map(|_| ()),
                BundledStep::Update => self.install(&wasm).map(|_| ()),
                BundledStep::Keep => Ok(()),
            };
            if let Err(e) = done {
                eprintln!("RequestTailor: bundled plugin {id} not installed: {e}");
                continue;
            }
            match (step, installed) {
                (BundledStep::Keep, None) => {}
                (BundledStep::Keep, Some(v)) => {
                    record.bundled.insert(id.clone(), v);
                }
                _ => {
                    record.bundled.insert(id.clone(), shipped.version.clone());
                }
            }
        }
        if *self.record(welcome) != record {
            self.save_record(&record)?;
            welcome.record = Some(record);
        }
        Ok(())
    }

    /// The first-start question: answered or not, and the bundled plugins that offer a choice.
    pub fn welcome(&self) -> Result<WelcomeState, AppError> {
        let mut welcome = lock(&self.welcome);
        self.ensure_bundled(&mut welcome)?;
        let record = self.record(&mut welcome).clone();
        let plugins = lock(&self.plugins);
        let choices = plugins
            .iter()
            .filter(|p| record.bundled.contains_key(p.id()))
            .filter_map(|p| {
                Some(WelcomeOffer {
                    plugin: p.id().to_owned(),
                    choice: p.info.manifest.contributes.welcome.clone()?,
                })
            })
            .collect();
        Ok(WelcomeState {
            done: record.done,
            choices,
        })
    }

    /// Enables the chosen bundled plugins (`[]`: none) and records the answer.
    pub fn choose_welcome(&self, chosen: &[String]) -> Result<(), AppError> {
        let offered = self.welcome()?.choices;
        if let Some(id) = chosen
            .iter()
            .find(|id| !offered.iter().any(|o| o.plugin == **id))
        {
            return Err(AppError::Plugin(format!("{id} is not a bundled plugin")));
        }
        for id in chosen {
            self.set_enabled(id, true)?;
        }
        let mut welcome = lock(&self.welcome);
        let record = self.record(&mut welcome);
        record.done = true;
        let record = record.clone();
        self.save_record(&record)
    }

    /// A new plugin, installed disabled (a bundled plugin before it is chosen).
    fn install_disabled(&self, wasm: &[u8]) -> Result<PluginInfo, AppError> {
        let mut plugin = match &self.dir {
            Some(dir) => {
                let plugin = dir.install(wasm)?;
                dir.set_enabled(plugin.id(), false)?;
                plugin
            }
            None => {
                let manifest = PluginHost::inspect(wasm)?;
                Plugin::compile(wasm, &manifest.permissions, false)?
            }
        };
        plugin.info.enabled = false;
        let info = PluginInfo::from(&plugin.info);
        let mut plugins = lock(&self.plugins);
        match plugins.iter_mut().find(|p| p.id() == plugin.id()) {
            Some(p) => *p = plugin,
            None => plugins.push(plugin),
        }
        Ok(info)
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        lock(&self.plugins)
            .iter()
            .map(|p| PluginInfo::from(&p.info))
            .collect()
    }

    pub fn inspect(wasm: &[u8]) -> Result<Manifest, AppError> {
        Ok(PluginHost::inspect(wasm)?)
    }

    /// Installs or updates (same id) a plugin and grants what its manifest requests; the UI
    /// asked the user first. An update keeps the position and the enabled state.
    pub fn install(&self, wasm: &[u8]) -> Result<PluginInfo, AppError> {
        let plugin = match &self.dir {
            Some(dir) => dir.install(wasm)?,
            None => {
                let manifest = PluginHost::inspect(wasm)?;
                let enabled = lock(&self.plugins)
                    .iter()
                    .find(|p| p.id() == manifest.id)
                    .is_none_or(|p| p.info.enabled);
                Plugin::compile(wasm, &manifest.permissions, enabled)?
            }
        };
        let info = PluginInfo::from(&plugin.info);
        let mut plugins = lock(&self.plugins);
        match plugins.iter_mut().find(|p| p.id() == plugin.id()) {
            Some(p) => *p = plugin,
            None => plugins.push(plugin),
        }
        Ok(info)
    }

    pub fn remove(&self, id: &str) -> Result<(), AppError> {
        let mut plugins = lock(&self.plugins);
        let i = plugins
            .iter()
            .position(|p| p.id() == id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        if let Some(dir) = &self.dir {
            dir.remove(id)?;
        }
        plugins.remove(i);
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), AppError> {
        let mut plugins = lock(&self.plugins);
        let plugin = plugins
            .iter_mut()
            .find(|p| p.id() == id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        if let Some(dir) = &self.dir {
            dir.set_enabled(id, enabled)?;
        }
        plugin.info.enabled = enabled;
        Ok(())
    }

    /// A host with the installed plugins, for one send or one call. Build it on the thread
    /// that uses it.
    pub fn host(&self) -> PluginHost {
        let storage = match &self.dir {
            Some(dir) => DesktopStorage::File(dir.storage()),
            None => DesktopStorage::Memory(MemoryStorage::default()),
        };
        let services = TransportServices::new(self.transport.clone(), storage).with_logger(
            Box::new(|plugin, level, message| {
                let level = match level {
                    NoteLevel::Info => "info",
                    NoteLevel::Warning => "warning",
                    NoteLevel::Error => "error",
                };
                eprintln!("RequestTailor: plugin {plugin} ({level}): {message}");
            }),
        );
        PluginHost::with_plugins(Box::new(services), lock(&self.plugins).clone())
    }

    /// The plugins of one send. `cookies` is the send's cookie jar, shared with plugin
    /// `http` calls in the session (wave 6).
    pub fn for_send<'a>(
        &'a self,
        collection: &str,
        cookies: crate::cookies::CookieSend<'a>,
    ) -> PluginSend<'a> {
        let host = self.host();
        PluginSend {
            host: host.has_hooks().then_some(host),
            parts: self,
            collection: collection.to_owned(),
            cookies,
        }
    }
}

/// The plugins of one send: the enabled plugins' hooks (in the send's cookie session), and
/// the response head recorded for response tabs.
pub struct PluginSend<'a> {
    host: Option<PluginHost>,
    parts: &'a PluginParts,
    collection: String,
    cookies: crate::cookies::CookieSend<'a>,
}

impl PluginSend<'_> {
    pub fn hooks(&self) -> Option<&dyn rt_core::services::RequestHooks> {
        Some(self)
    }

    fn recording<'s>(&'s self, session: Option<&'s dyn RequestHooks>) -> Recording<'s> {
        Recording {
            hooks: session,
            heads: &self.parts.heads,
            collection: &self.collection,
        }
    }

    fn session(&self) -> Option<rt_plugin::host::SessionHooks<'_>> {
        self.host.as_ref().map(|h| h.in_session(self.cookies.jar()))
    }
}

impl RequestHooks for PluginSend<'_> {
    fn before_send<'a>(
        &'a self,
        request: HttpRequest,
        secrets: &'a [String],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Result<(HttpRequest, Vec<Note>), String>> {
        Box::pin(async move {
            let session = self.session();
            let hooks = session.as_ref().map(|s| s as &dyn RequestHooks);
            self.recording(hooks)
                .before_send(request, secrets, context)
                .await
        })
    }

    fn after_response<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, Vec<Note>> {
        Box::pin(async move {
            let session = self.session();
            let hooks = session.as_ref().map(|s| s as &dyn RequestHooks);
            self.recording(hooks)
                .after_response(head, body, context)
                .await
        })
    }

    fn after_response_full<'a>(
        &'a self,
        head: &'a ResponseHead,
        body: &'a [u8],
        context: &'a HookContext,
    ) -> LocalBoxFuture<'a, AfterResponse> {
        Box::pin(async move {
            let session = self.session();
            let hooks = session.as_ref().map(|s| s as &dyn RequestHooks);
            self.recording(hooks)
                .after_response_full(head, body, context)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use rt_app::{SecretKey, SendInput};

    use super::*;

    #[derive(Default)]
    struct MemorySecrets(Mutex<HashMap<SecretKey, String>>);

    impl SecretStore for MemorySecrets {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, AppError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &SecretKey, value: &str) -> Result<(), AppError> {
            self.0.lock().unwrap().insert(key.clone(), value.to_owned());
            Ok(())
        }
        fn delete(&self, key: &SecretKey) -> Result<(), AppError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/rt-plugin/tests/fixtures")
            .join(name);
        std::fs::read(path).unwrap()
    }

    /// A fresh directory per call: tests run in parallel and may ask for the same name.
    fn temp(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rt-desktop-plugins-{name}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn parts_are_shared_between_threads() {
        assert_send_sync::<PluginParts>();
    }

    /// Wave 6 gate ("whoever does not pick SAP at the first start sees no SAP"): a bundled
    /// plugin is installed disabled, offered once, enabled only when chosen; the answer and
    /// the installation survive a restart, a removed bundled plugin stays removed, a newer
    /// bundled version updates the installed one.
    #[test]
    fn bundled_plugins_are_installed_disabled_until_chosen() {
        let data = temp("welcome-data");
        let bundled = temp("welcome-bundled");
        std::fs::write(bundled.join("demo.wasm"), fixture("demo-bundled.wasm")).unwrap();
        let start = || {
            let parts = PluginParts::new(Some(&data));
            parts.bundle_from(vec![temp_missing(), bundled.clone()], &["demo"]);
            parts
        };

        let parts = start();
        assert!(
            parts.list().is_empty(),
            "nothing is read before the question"
        );
        let w = parts.welcome().unwrap();
        assert!(!w.done);
        assert_eq!(w.choices.len(), 1);
        assert_eq!(w.choices[0].plugin, "demo");
        assert_eq!(w.choices[0].choice.label, "Demo extras");
        let list = parts.list();
        assert_eq!(list.len(), 1);
        assert!(!list[0].enabled && list[0].granted);
        assert!(!parts.host().has_hooks(), "disabled: no hooks, no tabs");

        // Not answered (the app was closed): asked again, still disabled, installed once.
        let parts = start();
        assert!(!parts.welcome().unwrap().done);
        assert_eq!(parts.list().len(), 1);
        assert!(!parts.list()[0].enabled);

        // "No extras": answered, nothing enabled; never asked again.
        parts.choose_welcome(&[]).unwrap();
        assert!(parts.welcome().unwrap().done);
        let parts = start();
        let w = parts.welcome().unwrap();
        assert!(w.done && w.choices.len() == 1);
        assert!(!parts.list()[0].enabled);
        assert!(data.join(WELCOME_FILE).is_file());

        // Only bundled plugins can be chosen; choosing one enables it.
        assert!(matches!(
            parts.choose_welcome(&["other".into()]),
            Err(AppError::Plugin(m)) if m.contains("not a bundled plugin")
        ));
        parts.choose_welcome(&["demo".into()]).unwrap();
        assert!(parts.list()[0].enabled);
        assert!(start().list()[0].enabled, "persisted in the plugin folder");

        // A newer bundled version updates it and keeps it enabled.
        std::fs::write(bundled.join("demo.wasm"), fixture("demo-v2.wasm")).unwrap();
        let parts = start();
        parts.welcome().unwrap();
        let list = parts.list();
        assert_eq!(list[0].manifest.version, "2.0.0");
        assert!(list[0].enabled);

        // Removed by the user: stays removed at the next start.
        parts.remove("demo").unwrap();
        let parts = start();
        parts.welcome().unwrap();
        assert!(parts.list().is_empty());
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&bundled);
    }

    /// A first start that picks the bundled plugin enables it at once.
    #[test]
    fn choosing_a_bundled_plugin_enables_it() {
        let bundled = temp("choose-bundled");
        std::fs::write(bundled.join("demo.wasm"), fixture("demo-bundled.wasm")).unwrap();
        // Without a data folder (nothing persisted) the same rules apply in memory.
        let parts = PluginParts::new(None);
        parts.bundle_from(vec![bundled.clone()], &["demo"]);
        assert!(!parts.welcome().unwrap().done);
        parts.choose_welcome(&["demo".into()]).unwrap();
        assert!(parts.welcome().unwrap().done);
        assert!(parts.list()[0].enabled);
        assert!(parts.host().has_hooks());
        let _ = std::fs::remove_dir_all(&bundled);
    }

    fn temp_missing() -> PathBuf {
        std::env::temp_dir().join(format!("rt-desktop-no-such-dir-{}", std::process::id()))
    }

    #[test]
    fn install_update_disable_remove_persist() {
        let data = temp("lifecycle");
        let parts = PluginParts::new(Some(&data));
        assert!(parts.list().is_empty());
        let info = parts.install(&fixture("demo.wasm")).unwrap();
        assert!(info.granted && info.enabled);
        parts.set_enabled("demo", false).unwrap();
        let info = parts.install(&fixture("demo-v2.wasm")).unwrap();
        assert_eq!(info.manifest.version, "2.0.0");
        assert!(!info.enabled, "an update keeps the plugin disabled");

        // Shared with the CLI and the next start: read back from the folder.
        let again = PluginParts::new(Some(&data));
        assert_eq!(again.list(), parts.list());
        assert!(data.join("plugins/demo.wasm").is_file());

        assert!(matches!(
            parts.install(b"junk"),
            Err(AppError::Plugin(m)) if m.contains("not a RequestTailor plugin")
        ));
        parts.remove("demo").unwrap();
        assert!(parts.list().is_empty());
        assert!(parts.remove("demo").is_err());
        assert!(PluginParts::new(Some(&data)).list().is_empty());
        let _ = std::fs::remove_dir_all(&data);
    }

    fn collection(base_url: &str) -> PathBuf {
        let dir = temp("collection");
        std::fs::write(
            dir.join("opencollection.yml"),
            "opencollection: \"1.0.0\"\ninfo:\n  name: Plugins\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("get.yml"),
            format!(
                "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: {base_url}/echo/plugins\n"
            ),
        )
        .unwrap();
        dir
    }

    /// Wave 5 gate: the official example plugin (`plugins/example`, built by
    /// `node scripts/build-plugins.mjs`) runs unchanged here, as in the web app and the CLI.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_example_plugin_runs_unchanged() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/plugins/example.wasm");
        let Ok(wasm) = std::fs::read(&path) else {
            eprintln!(
                "SKIPPED the_example_plugin_runs_unchanged: run `node scripts/build-plugins.mjs` first"
            );
            return;
        };
        let addr = rt_testserver::spawn_local().await;
        let dir = collection(&format!("http://{addr}"));
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&dir).unwrap();
        let info = core.services.plugins.install(&wasm).unwrap();
        assert_eq!(info.manifest.id, "example");

        let stored = core
            .send(
                "get.yml",
                SendInput {
                    environment: None,
                    vars: Vec::new(),
                    secrets: Vec::new(),
                },
            )
            .await
            .unwrap();
        let sent = stored.report.request.as_ref().unwrap();
        assert!(
            sent.headers
                .iter()
                .any(|(n, v)| n == "X-Request-ID" && v.starts_with("rt-"))
        );
        let notes: Vec<_> = stored
            .report
            .notes
            .iter()
            .map(|n| n.source.as_str())
            .collect();
        assert_eq!(notes, ["plugin:example", "plugin:example"]);

        let target = TabTarget {
            request_path: "get.yml".into(),
            response_id: stored.response_id,
        };
        let view = render_tab(
            &core,
            "example".into(),
            "summary".into(),
            None,
            target,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!view.view.blocks.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sends_run_plugins_and_tabs_see_the_response() {
        let addr = rt_testserver::spawn_local().await;
        let dir = collection(&format!("http://{addr}"));
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.open(&dir).unwrap();
        core.services
            .plugins
            .install(&fixture("demo.wasm"))
            .unwrap();

        let stored = core
            .send(
                "get.yml",
                SendInput {
                    environment: None,
                    vars: Vec::new(),
                    secrets: Vec::new(),
                },
            )
            .await
            .unwrap();
        let notes: Vec<_> = stored
            .report
            .notes
            .iter()
            .map(|n| n.text.as_str())
            .collect();
        assert_eq!(
            notes,
            [
                "Demo plugin added X-Demo-Plugin",
                "Demo plugin saw the response"
            ]
        );
        let sent = stored.report.request.as_ref().unwrap();
        assert!(
            sent.headers
                .iter()
                .any(|(n, v)| n == "X-Demo-Plugin" && v == "1.0.0")
        );

        let target = TabTarget {
            request_path: "get.yml".into(),
            response_id: stored.response_id,
        };
        let view = render_tab(
            &core,
            "demo".into(),
            "demo-response".into(),
            None,
            target.clone(),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(view.view.blocks.len(), 4);
        assert_eq!(
            core.services
                .plugins
                .heads
                .get(&core.collection_id().unwrap(), "get.yml")
                .map(|h| h.status),
            Some(200)
        );
        let view = render_tab(
            &core,
            "demo".into(),
            "demo-request".into(),
            Some(("hello".into(), Vec::new())),
            target,
            Some(serde_json::json!({"rendered": true})),
            None,
        )
        .await
        .unwrap();
        assert_eq!(view.view.blocks.len(), 1);

        // Disabled: the next send runs no plugin, and its tabs refuse.
        core.services.plugins.set_enabled("demo", false).unwrap();
        let stored = core
            .send(
                "get.yml",
                SendInput {
                    environment: None,
                    vars: Vec::new(),
                    secrets: Vec::new(),
                },
            )
            .await
            .unwrap();
        assert!(stored.report.notes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plugin_importers_feed_the_import_flow() {
        let core = Core::new(MemorySecrets::default()).unwrap();
        core.services
            .plugins
            .install(&fixture("demo.wasm"))
            .unwrap();
        let header = serde_json::json!({
            "plugin": "demo", "importer": "demo", "files": [{"name": "x.demo", "size": 2}]
        })
        .to_string();
        let mut body = (header.len() as u32).to_le_bytes().to_vec();
        body.extend_from_slice(header.as_bytes());
        body.extend_from_slice(b"hi");
        let plan = analyze_with_plugin(&core, &body).await.unwrap();
        assert_eq!(plan.collection_name, "Demo import");
        assert_eq!(plan.files, 2);
        assert_eq!(plan.report.format, rt_format::import::ImportFormat::Plugin);

        let mut wrong = (header.len() as u32).to_le_bytes().to_vec();
        wrong.extend_from_slice(header.replace("\"demo\",", "\"nope\",").as_bytes());
        assert!(analyze_with_plugin(&core, &wrong).await.is_err());
    }
}
