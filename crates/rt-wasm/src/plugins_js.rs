// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin host in the browser (wave 5, strand A): `rt_plugin::host::PluginHost` behind
//! the JS class `Plugins`, with host services over `fetch` (or the companion) and browser
//! storage. The same interpreter as natively runs inside the page's WebAssembly.
//!
//! The web shell (`apps/web/src/plugins.ts`) keeps the modules in IndexedDB and loads them
//! here once per page (`Plugins.load`); the compiled plugins live in this module until the
//! page closes. The services object it passes (to `Workspace.send` as field `plugins`, and
//! to `renderTab` / `importFiles` directly) is
//! `{ collection: string, companion: { url, token } | null, storage: { get(id, key):
//! Promise<unknown>, set(id, key, value | undefined): Promise<void> } }`.
//!
//! - `Plugins.inspect(wasm)` → `Manifest`; `Plugins.load(wasm, granted, enabled)` →
//!   `PluginInfo` (replaces a plugin with the same id); `unload(id)`, `setEnabled(id, on)`,
//!   `list()` → `PluginInfo[]`.
//! - `Plugins.renderTab(plugin, tab, action | null, input, target, spec, body | null, state,
//!   services, scope?)` → `Promise<ViewReply>`: `spec` and `body` come from the workspace,
//!   `scope` (wave 6) from `Workspace.pluginScope(requestPath, environment)`.
//! - `Plugins.importFiles(plugin, importer, files, services)` → `Promise<ImportReply>`, for
//!   `Importer.addPluginImport`.

use std::rc::Rc;
use std::sync::{LazyLock, Mutex, MutexGuard};

use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use rt_app::plugins::{
    Permissions, Plugin, PluginError, PluginHost, PluginInfo, Recording, ResponseHeads, TabTarget,
    plugin_files,
};
use rt_app::{AppError, import::SourceFile};
use rt_core::model::RequestSpec;
use rt_core::services::{
    AfterResponse, CookieJar, HookContext, LocalBoxFuture, Note, NoteLevel, RequestHooks,
};
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use rt_core::transport::{HttpRequest, Response, ResponseHead, Transport, TransportError};
use rt_plugin::host::services::{PluginStorage, TransportServices};
use rt_plugin::host::{SessionHooks, TabScope};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::Companion;
use crate::workspace_js::{app_error, from_js, to_js};

/// The page's plugins, in installation order.
static PLUGINS: LazyLock<Mutex<Vec<Plugin>>> = LazyLock::new(|| Mutex::new(Vec::new()));
/// Response heads for response tabs.
static HEADS: LazyLock<ResponseHeads> = LazyLock::new(ResponseHeads::new);

fn plugins() -> MutexGuard<'static, Vec<Plugin>> {
    PLUGINS.lock().unwrap_or_else(|p| p.into_inner())
}

fn plugin_error(e: PluginError) -> JsValue {
    app_error(AppError::from(e))
}

/// `fetch`, or the companion when the page is paired with one.
enum WebTransport {
    Fetch(FetchTransport),
    Companion(CompanionTransport),
}

impl Transport for WebTransport {
    #[allow(clippy::manual_async_fn)]
    fn send(
        &self,
        request: HttpRequest,
    ) -> impl std::future::Future<Output = Result<Response, TransportError>> {
        async move {
            match self {
                Self::Fetch(t) => t.send(request).await,
                Self::Companion(t) => t.send(request).await,
            }
        }
    }
}

/// `storage.get` / `storage.set` of the services object; without them nothing is kept.
struct JsStorage {
    get: Option<Function>,
    set: Option<Function>,
}

async fn settle(value: Result<JsValue, JsValue>) -> Option<JsValue> {
    let value = value.ok()?;
    match value.dyn_into::<Promise>() {
        Ok(p) => JsFuture::from(p).await.ok(),
        Err(v) => Some(v),
    }
}

impl PluginStorage for JsStorage {
    fn get<'a>(&'a self, plugin: &'a str, key: &'a str) -> LocalBoxFuture<'a, Option<Value>> {
        Box::pin(async move {
            let f = self.get.as_ref()?;
            let v = settle(f.call2(&JsValue::NULL, &plugin.into(), &key.into())).await?;
            if v.is_undefined() || v.is_null() {
                return None;
            }
            from_js(&v, "storage value").ok()
        })
    }

    fn set<'a>(
        &'a self,
        plugin: &'a str,
        key: &'a str,
        value: Option<Value>,
    ) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            let Some(f) = self.set.as_ref() else { return };
            let value = match value {
                Some(v) => to_js(&v).unwrap_or(JsValue::UNDEFINED),
                None => JsValue::UNDEFINED,
            };
            let args = Array::of3(&plugin.into(), &key.into(), &value);
            let _ = settle(f.apply(&JsValue::NULL, &args)).await;
        })
    }
}

/// What the services object says.
struct Parts {
    collection: String,
    companion: Option<Companion>,
    storage: JsStorage,
}

fn field(object: &JsValue, name: &str) -> JsValue {
    if object.is_object() {
        Reflect::get(object, &name.into()).unwrap_or(JsValue::UNDEFINED)
    } else {
        JsValue::UNDEFINED
    }
}

impl Parts {
    fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let companion = field(services, "companion");
        let storage = field(services, "storage");
        Ok(Self {
            collection: field(services, "collection")
                .as_string()
                .unwrap_or_default(),
            companion: if companion.is_object() {
                Some(from_js(&companion, "plugins.companion")?)
            } else {
                None
            },
            storage: JsStorage {
                get: field(&storage, "get").dyn_into().ok(),
                set: field(&storage, "set").dyn_into().ok(),
            },
        })
    }

    fn host(self) -> PluginHost {
        let transport = match self.companion {
            Some(c) => WebTransport::Companion(CompanionTransport::new(c.url, c.token)),
            None => WebTransport::Fetch(FetchTransport::new()),
        };
        let services = TransportServices::new(Some(transport), self.storage).with_logger(Box::new(
            |plugin, level, message| {
                let line = format!("RequestTailor plugin {plugin}: {message}");
                match level {
                    NoteLevel::Info => web_sys_log(&line, false),
                    _ => web_sys_log(&line, true),
                }
            },
        ));
        PluginHost::with_plugins(Box::new(services), plugins().clone())
    }
}

fn web_sys_log(line: &str, warn: bool) {
    let console = Reflect::get(&js_sys::global(), &"console".into()).ok();
    if let Some(console) = console {
        let name = if warn { "warn" } else { "log" };
        if let Ok(f) = Reflect::get(&console, &name.into()).and_then(|f| f.dyn_into::<Function>()) {
            let _ = f.call1(&console, &line.into());
        }
    }
}

#[wasm_bindgen(js_name = Plugins)]
pub struct PluginsJs;

#[wasm_bindgen(js_class = Plugins)]
impl PluginsJs {
    /// The manifest of a `.wasm` file, without installing it.
    pub fn inspect(wasm: &[u8]) -> Result<JsValue, JsValue> {
        to_js(&PluginHost::inspect(wasm).map_err(plugin_error)?)
    }

    /// Compiles a plugin and keeps it for this page; replaces one with the same id.
    pub fn load(wasm: &[u8], granted: JsValue, enabled: bool) -> Result<JsValue, JsValue> {
        let granted: Permissions = from_js(&granted, "granted")?;
        let plugin = Plugin::compile(wasm, &granted, enabled).map_err(plugin_error)?;
        let info = PluginInfo::from(&plugin.info);
        let mut list = plugins();
        match list.iter_mut().find(|p| p.id() == plugin.id()) {
            Some(p) => *p = plugin,
            None => list.push(plugin),
        }
        to_js(&info)
    }

    pub fn unload(id: &str) {
        plugins().retain(|p| p.id() != id);
    }

    #[wasm_bindgen(js_name = setEnabled)]
    pub fn set_enabled(id: &str, enabled: bool) -> Result<(), JsValue> {
        let mut list = plugins();
        let plugin = list
            .iter_mut()
            .find(|p| p.id() == id)
            .ok_or_else(|| plugin_error(PluginError::NotFound(id.to_owned())))?;
        plugin.info.enabled = enabled;
        Ok(())
    }

    pub fn list() -> Result<JsValue, JsValue> {
        let infos: Vec<PluginInfo> = plugins()
            .iter()
            .map(|p| PluginInfo::from(&p.info))
            .collect();
        to_js(&infos)
    }

    /// Renders a tab (`action: null`) or runs one of its actions. `spec` is the request as in
    /// the editor, `body` the response body for response tabs. `scope` (wave 6): the tab's
    /// context and the URLs `{request}` covers, from `Workspace.pluginScope` (`undefined`:
    /// none).
    #[wasm_bindgen(js_name = renderTab)]
    #[allow(clippy::too_many_arguments)]
    pub fn render_tab(
        plugin: String,
        tab: String,
        action: Option<String>,
        input: JsValue,
        target: JsValue,
        spec: JsValue,
        body: Option<Vec<u8>>,
        state: JsValue,
        services: JsValue,
        scope: JsValue,
    ) -> Result<Promise, JsValue> {
        let target: TabTarget = from_js(&target, "target")?;
        let scope: TabScope = if scope.is_undefined() || scope.is_null() {
            TabScope::default()
        } else {
            from_js(&scope, "scope")?
        };
        // Collection tabs (wave 6) have no request: `null` is the empty one.
        let spec: RequestSpec = if spec.is_null() || spec.is_undefined() {
            RequestSpec::default()
        } else {
            from_js(&spec, "spec")?
        };
        let input: Vec<(String, String)> = if input.is_undefined() || input.is_null() {
            Vec::new()
        } else {
            from_js(&input, "input")?
        };
        let state: Option<Value> = if state.is_undefined() {
            None
        } else {
            from_js(&state, "state")?
        };
        let parts = Parts::from_js(&services)?;
        let response =
            body.map(|b| HEADS.tab_response(&parts.collection, &target.request_path, &b));
        let host = parts.host();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let action = action.as_deref().map(|a| (a, input));
            let reply = host
                .tab_with(&plugin, &tab, action, target, spec, response, state, scope)
                .await
                .map_err(plugin_error)?;
            to_js(&reply)
        }))
    }

    /// Runs a plugin importer on `files` (`{ name, data: Uint8Array }[]`): `detect`, then
    /// `import`. Resolves with the `ImportReply` for `Importer.addPluginImport`.
    #[wasm_bindgen(js_name = importFiles)]
    pub fn import_files(
        plugin: String,
        importer: String,
        files: Array,
        services: JsValue,
    ) -> Result<Promise, JsValue> {
        let mut owned = Vec::new();
        for entry in files.iter() {
            let name = Reflect::get(&entry, &"name".into())?
                .as_string()
                .ok_or_else(|| {
                    JsValue::from(js_sys::TypeError::new("files[].name must be a string"))
                })?;
            let data = Reflect::get(&entry, &"data".into())?
                .dyn_into::<Uint8Array>()
                .map_err(|_| {
                    JsValue::from(js_sys::TypeError::new("files[].data must be a Uint8Array"))
                })?;
            owned.push((name, data.to_vec()));
        }
        let sources: Vec<SourceFile<'_>> = owned
            .iter()
            .map(|(name, data)| SourceFile { name, data })
            .collect();
        let input = plugin_files(&sources);
        let host = Parts::from_js(&services)?.host();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            if !host
                .detect(&plugin, &importer, input.clone())
                .await
                .map_err(plugin_error)?
            {
                return Err(app_error(AppError::Import(format!(
                    "the importer of plugin {plugin} does not recognize these files"
                ))));
            }
            let reply = host
                .import(&plugin, &importer, input)
                .await
                .map_err(plugin_error)?;
            to_js(&reply)
        }))
    }
}

/// The plugin part of one send, from the `plugins` field of the services object the web
/// shell passes to `Workspace.send`. `cookies`: the send's jar, for `http` with
/// `session: true` (wave 6).
pub struct PluginSend {
    host: Option<PluginHost>,
    collection: String,
    cookies: Option<Rc<dyn CookieJar>>,
}

impl PluginSend {
    pub fn from_js(
        services: &JsValue,
        cookies: Option<Rc<dyn CookieJar>>,
    ) -> Result<Self, JsValue> {
        let value = field(services, "plugins");
        if !value.is_object() {
            return Ok(Self {
                host: None,
                collection: String::new(),
                cookies,
            });
        }
        let parts = Parts::from_js(&value)?;
        let collection = parts.collection.clone();
        let host = parts.host();
        Ok(Self {
            host: host.has_hooks().then_some(host),
            collection,
            cookies,
        })
    }

    pub fn hooks(&self) -> Option<&dyn rt_core::services::RequestHooks> {
        Some(self)
    }

    fn recording<'s>(&'s self, session: Option<&'s dyn RequestHooks>) -> Recording<'s> {
        Recording {
            hooks: session,
            heads: &HEADS,
            collection: &self.collection,
        }
    }

    fn session(&self) -> Option<SessionHooks<'_>> {
        self.host
            .as_ref()
            .map(|h| h.in_session(self.cookies.as_deref()))
    }
}

impl RequestHooks for PluginSend {
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
