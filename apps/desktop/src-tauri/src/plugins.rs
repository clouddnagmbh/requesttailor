// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plugins in the desktop app (wave 5, strand A): installed modules live in the data folder's
//! `plugins/` directory (shared with the CLI, see `rt_plugin::host::dir`), a
//! `rt_plugin::host::PluginHost` runs them, and `Core::send` passes it to `send_with` as
//! `RequestHooks`. Commands: `plugin_list`, `plugin_inspect`, `plugin_install` (raw body: the
//! `.wasm` bytes), `plugin_remove`, `plugin_set_enabled`, `plugin_render_tab`,
//! `plugin_action`, `import_analyze_plugin` (raw body as `import_analyze`, header with
//! `plugin` and `importer`).
//!
//! The compiled plugins are kept in [`PluginParts`] (`Send + Sync`); a `PluginHost` is not
//! `Send`, so every send, tab call and import builds one on its blocking thread from them.

use std::sync::{Arc, Mutex, MutexGuard};

use rt_app::import::SourceFile;
use rt_app::plugins::{
    Manifest, Plugin, PluginError, PluginHost, PluginInfo, Recording, ResponseHeads, TabTarget,
    ViewReply, plugin_files, plugin_import,
};
use rt_app::{AppError, ImportPlan, SecretStore};
use rt_core::services::{HookContext, LocalBoxFuture, Note, NoteLevel, RequestHooks};
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
) -> Result<ViewReply, CommandError> {
    Ok(render_tab(&core, plugin, tab, None, target, state).await?)
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
) -> Result<ViewReply, CommandError> {
    Ok(render_tab(&core, plugin, tab, Some((action, input)), target, state).await?)
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
/// in the editor (unsaved changes included) and the response in the response store.
pub async fn render_tab<S: SecretStore>(
    core: &Core<S>,
    plugin: String,
    tab: String,
    action: Option<(String, Vec<(String, String)>)>,
    target: TabTarget,
    state: Option<Value>,
) -> Result<ViewReply, AppError> {
    let spec = core.get_request(&target.request_path)?;
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
                .tab(&plugin, &tab, action, target, spec, response, state)
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

/// Installed plugins and their host, kept by `Core` (`services::ServiceParts`).
pub struct PluginParts {
    /// `None`: nothing is persisted (tests); installs live until the app quits.
    dir: Option<PluginDir>,
    /// Installation order.
    plugins: Mutex<Vec<Plugin>>,
    transport: Option<NativeTransport>,
    /// Response heads for response tabs.
    pub heads: ResponseHeads,
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
        }
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

    pub fn for_send(&self, collection: &str, environment: Option<&str>) -> PluginSend<'_> {
        let _ = environment;
        let host = self.host();
        PluginSend {
            host: host.has_hooks().then_some(host),
            parts: self,
            collection: collection.to_owned(),
        }
    }
}

/// The plugins of one send: the enabled plugins' hooks, and the response head recorded for
/// response tabs.
pub struct PluginSend<'a> {
    host: Option<PluginHost>,
    parts: &'a PluginParts,
    collection: String,
}

impl PluginSend<'_> {
    pub fn hooks(&self) -> Option<&dyn rt_core::services::RequestHooks> {
        Some(self)
    }

    fn recording(&self) -> Recording<'_> {
        Recording {
            hooks: self.host.as_ref().map(|h| h as &dyn RequestHooks),
            heads: &self.parts.heads,
            collection: &self.collection,
        }
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
            self.recording()
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
        Box::pin(async move { self.recording().after_response(head, body, context).await })
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

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rt-desktop-plugins-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn parts_are_shared_between_threads() {
        assert_send_sync::<PluginParts>();
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
