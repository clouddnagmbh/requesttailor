// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-wasm`: the core as WebAssembly, for the web app and the browser conformance suite.
//!
//! # JavaScript API (wasm32 only)
//!
//! ```text
//! send(files, requestPath, options?) -> Promise<SendReport>
//! ```
//!
//! - `files`: the collection's files as an object or `Map` from path (relative to the
//!   collection root, `/`-separated, e.g. `"users/get-user.yml"`) to `string` (UTF-8) or
//!   `Uint8Array`. Body files referenced by requests are read from here too.
//! - `requestPath`: the request file's path in `files`.
//! - `options`: `{ environment?: string, vars?: Record<string, string>,
//!   globals?: Record<string, string>, bodyLimit?: number }`. `vars` are runtime variables
//!   (highest precedence, like `rt send --var`), `globals` the lowest layer.
//!
//! Resolves with the `SendReport` as a plain object: exactly the JSON `rt send` prints
//! (`JSON.parse` of the same serialization). Errors of the send itself (request not found,
//! preparation, transport) are in the report, as in the CLI. Rejects only when the input is
//! unusable: a `TypeError` for bad arguments, an `Error` named `FormatError` when the files
//! are not a valid collection.
//!
//! Requests go out through the browser's `fetch` (`FetchTransport`), so CORS applies.
//!
//! ```text
//! Workspace.open(files) -> Workspace
//! ```
//!
//! The web app's command layer: an open collection (`rt_app::Workspace`) plus the bodies of
//! its responses (`rt_app::ResponseStore`). `files` as for `send`. Methods mirror the UI's
//! `Backend` (`apps/ui/src/commands/backend.ts`); values go in and out as plain objects with
//! exactly the serde JSON of the Rust types (`apps/ui/src/commands/types.ts`):
//!
//! ```text
//! name() -> string                        tree() -> TreeNode
//! request(path) -> RequestSpec             createRequest(folder, name) -> string
//! updateRequest(path, spec)                dirty() -> string[]
//! prepareSave() -> PreparedSave            commit(prepared)
//! environments() -> Environment[]          updateEnvironment(env)
//! deleteEnvironment(name)                  preview(path, input) -> Preview
//! send(path, input, companion?) -> Promise<StoredResponse>
//! responseLines(id, start, count) -> LineSlice
//! responseBytes(id, offset, length) -> Uint8Array
//! filterJson(id, expression) -> string     releaseResponse(id)
//! ```
//!
//! Wave 4 (one file each): `search(query, limit) -> SearchHit[]` on `Workspace`;
//! `textDiff(path, old, new) -> GitDiff` (`git_js.rs`); `Importer` with
//! `analyze(files, format) -> ImportPlan` (`files`: `{ name, data: Uint8Array }[]`), `take(id) -> FileChange[]`, `discard(id)`
//! (`import_js.rs`); history helpers (`history_js.rs`, strand E).
//!
//! Wave 7: the collection runner on `Workspace` (`runPlan`, `run`) and `parseData`
//! (`runner_js.rs`, strand D).
//!
//! Wave 14: variable completion on `Workspace` (`variables`, `defineVariable`,
//! `variables_js.rs`). Wave 16: `folderScripts`, `setFolderScripts` (`folder_scripts_js.rs`).
//!
//! Saving is two-step so a failed write keeps the changes unsaved: `prepareSave()` returns
//! `{ changes: FileChange[] }` without touching the workspace; after the shell has written the
//! files it calls `commit(prepared)`, which clears `dirty`.
//!
//! `input` is `rt_app::SendInput` (`{ environment, vars, secrets }`): the web shell reads the
//! secret values from its browser store and passes them here. `companion` is
//! `{ url, token }` to send through `rt agent` (`CompanionTransport`), or `null`/`undefined`
//! for the browser's `fetch`. Transport failures, including `blocked`, are in
//! `report.error` of the result. Other errors are thrown as `Error`s named `AppError` with a
//! `kind` property (the serialized `AppError` kind, e.g. `request-not-found`) and a readable
//! `message`; bad arguments throw a `TypeError`.

use std::collections::BTreeMap;

use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{SendOptions, SendReport, send_with};
use rt_core::transport::Transport;
use rt_format::{FileSet, FormatError, read_collection};
use serde::Deserialize;

/// The `options` argument of `send`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub environment: Option<String>,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default)]
    pub globals: BTreeMap<String, String>,
    pub body_limit: Option<usize>,
}

impl Options {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("invalid options: {e}"))
    }
}

/// Body files from the in-memory file set. Same rules as the CLI's file-system source:
/// relative, `/`-separated, no `..`, no absolute paths.
#[derive(Debug, Clone, Copy)]
pub struct MemFiles<'a>(pub &'a FileSet);

impl FileSource for MemFiles<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        if path.starts_with(['/', '\\']) || path.contains(':') {
            return Err("absolute paths are not allowed".into());
        }
        let mut parts = Vec::new();
        for part in path.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => return Err("path leaves the collection folder".into()),
                p => parts.push(p),
            }
        }
        let key = parts.join("/");
        self.0
            .files
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("no file {key}"))
    }
}

/// Reads the collection from `files` and sends one request. The Rust form of the JS `send`.
pub async fn send_files<T: Transport>(
    files: &FileSet,
    request_path: &str,
    options: &Options,
    transport: &T,
) -> Result<SendReport, FormatError> {
    let doc = read_collection(files)?;
    let input = PrepareInput {
        environment: options.environment.as_deref(),
        globals: options.globals.clone().into_iter().collect(),
        runtime: options.vars.clone().into_iter().collect(),
    };
    let send_options = SendOptions {
        iteration: None,
        body_limit: options
            .body_limit
            .unwrap_or(SendOptions::default().body_limit),
    };
    // Wave 7: scripts run (as in `rt send`; the caller chose the files).
    let sandbox = rt_script::Sandbox::default();
    let services = rt_core::services::Services {
        scripts: Some(&sandbox),
        ..rt_core::services::Services::none()
    };
    Ok(send_with(
        doc.model(),
        request_path,
        &input,
        &MemFiles(files),
        transport,
        &send_options,
        &services,
    )
    .await)
}

#[cfg(target_arch = "wasm32")]
mod js {
    use js_sys::{Array, JSON, Map, Object, Uint8Array};
    use rt_core::transport::fetch::FetchTransport;
    use rt_format::FileSet;
    use wasm_bindgen::prelude::*;

    use super::{Options, send_files};

    pub(crate) fn type_error(msg: impl AsRef<str>) -> JsValue {
        js_sys::TypeError::new(msg.as_ref()).into()
    }

    pub(super) fn file_set(files: &JsValue) -> Result<FileSet, JsValue> {
        let entries = if files.is_instance_of::<Map>() {
            Array::from(files)
        } else if files.is_object() {
            Object::entries(files.unchecked_ref())
        } else {
            return Err(type_error("files must be an object or a Map"));
        };
        let mut set = FileSet::default();
        for entry in entries.iter() {
            let entry: Array = entry.unchecked_into();
            let path = entry
                .get(0)
                .as_string()
                .ok_or_else(|| type_error("file paths must be strings"))?;
            let value = entry.get(1);
            let bytes = if let Some(text) = value.as_string() {
                text.into_bytes()
            } else if value.is_instance_of::<Uint8Array>() {
                value.unchecked_into::<Uint8Array>().to_vec()
            } else {
                return Err(type_error(format!(
                    "file {path}: expected a string or Uint8Array"
                )));
            };
            set.files.insert(path, bytes);
        }
        Ok(set)
    }

    fn options(options: &JsValue) -> Result<Options, JsValue> {
        if options.is_undefined() || options.is_null() {
            return Ok(Options::default());
        }
        let json = JSON::stringify(options)
            .map_err(|_| type_error("options must be JSON-serializable"))?;
        Options::from_json(&String::from(json)).map_err(type_error)
    }

    /// See the crate documentation.
    #[wasm_bindgen(js_name = send)]
    pub async fn send_js(
        files: JsValue,
        #[wasm_bindgen(js_name = requestPath)] request_path: String,
        options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let files = file_set(&files)?;
        let options = self::options(&options)?;
        let report = send_files(&files, &request_path, &options, &FetchTransport::new())
            .await
            .map_err(|e| {
                let err = js_sys::Error::new(&e.to_string());
                err.set_name("FormatError");
                JsValue::from(err)
            })?;
        let json = serde_json::to_string(&report).expect("report serializes");
        JSON::parse(&json)
    }
}

#[cfg(target_arch = "wasm32")]
pub use js::send_js;

/// Bytes kept per response by the web workspace (the response store keeps the full body).
pub const BODY_LIMIT: usize = 256 * 1024 * 1024;

/// Where the web workspace sends through the companion (`rt agent`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    pub url: String,
    pub token: String,
}

/// `kind` (as serialized, e.g. `request-not-found`) and readable message of an `AppError`.
pub fn error_parts(e: &rt_app::AppError) -> (String, String) {
    let kind = serde_json::to_value(e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "shell".to_owned());
    (kind, e.to_string())
}

#[cfg(target_arch = "wasm32")]
mod workspace_js {
    use std::cell::RefCell;
    use std::rc::Rc;

    use js_sys::{JSON, Promise, Reflect, Uint8Array};
    use rt_app::{AppError, ResponseStore, SendInput, Workspace};
    use rt_core::send::SendOptions;
    use rt_core::transport::companion::CompanionTransport;
    use rt_core::transport::fetch::FetchTransport;
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use wasm_bindgen::prelude::*;

    use super::{BODY_LIMIT, Companion, error_parts};

    pub(crate) fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
        let json =
            serde_json::to_string(value).map_err(|e| named_error("shell", &e.to_string()))?;
        JSON::parse(&json)
    }

    pub(crate) fn from_js<T: DeserializeOwned>(value: &JsValue, what: &str) -> Result<T, JsValue> {
        let json = JSON::stringify(value)
            .map_err(|_| type_error(format!("{what} must be JSON-serializable")))?;
        serde_json::from_str(&String::from(json))
            .map_err(|e| type_error(format!("invalid {what}: {e}")))
    }

    fn type_error(msg: String) -> JsValue {
        js_sys::TypeError::new(&msg).into()
    }

    pub(crate) fn named_error(kind: &str, message: &str) -> JsValue {
        let err = js_sys::Error::new(message);
        err.set_name("AppError");
        let _ = Reflect::set(&err, &"kind".into(), &kind.into());
        err.into()
    }

    pub(crate) fn app_error(e: AppError) -> JsValue {
        let (kind, message) = error_parts(&e);
        named_error(&kind, &message)
    }

    /// JavaScript numbers are f64; ids and offsets are non-negative integers.
    pub(crate) fn int(value: f64) -> Result<u64, JsValue> {
        if value.is_finite() && value >= 0.0 && value.fract() == 0.0 {
            Ok(value as u64)
        } else {
            Err(type_error(format!(
                "expected a non-negative integer, got {value}"
            )))
        }
    }

    struct Inner {
        workspace: Workspace,
        responses: ResponseStore,
    }

    /// See the crate documentation. State sits in an `Rc<RefCell<_>>` so `send` awaits
    /// without holding a borrow: other calls stay possible while a request runs.
    #[wasm_bindgen(js_name = Workspace)]
    pub struct WorkspaceJs {
        inner: Rc<RefCell<Inner>>,
    }

    impl WorkspaceJs {
        /// The open workspace, for the per-strand modules (wave 7: `runner_js`).
        pub(crate) fn workspace(&self) -> std::cell::Ref<'_, Workspace> {
            std::cell::Ref::map(self.inner.borrow(), |i| &i.workspace)
        }

        /// The kept responses, for the per-strand modules (wave 8: `ai_js`).
        pub(crate) fn responses(&self) -> std::cell::Ref<'_, ResponseStore> {
            std::cell::Ref::map(self.inner.borrow(), |i| &i.responses)
        }

        /// Another handle on the same workspace, for work that outlives the call (wave 18).
        pub(crate) fn handle(&self) -> WorkspaceJs {
            WorkspaceJs {
                inner: Rc::clone(&self.inner),
            }
        }

        /// The kept responses, to add one (wave 18: `paging_js` keeps the merged page run).
        pub(crate) fn responses_mut(&self) -> std::cell::RefMut<'_, ResponseStore> {
            std::cell::RefMut::map(self.inner.borrow_mut(), |i| &mut i.responses)
        }

        /// The open workspace for edits (wave 8: `agents_js`; saved MCP tests are staged in it, `mcp_js`).
        pub(crate) fn workspace_mut(&self) -> std::cell::RefMut<'_, Workspace> {
            std::cell::RefMut::map(self.inner.borrow_mut(), |i| &mut i.workspace)
        }
    }

    #[wasm_bindgen(js_class = Workspace)]
    impl WorkspaceJs {
        pub fn open(files: JsValue) -> Result<WorkspaceJs, JsValue> {
            let files = super::js::file_set(&files)?;
            let workspace = Workspace::open(files).map_err(app_error)?;
            Ok(Self {
                inner: Rc::new(RefCell::new(Inner {
                    workspace,
                    responses: ResponseStore::new(),
                })),
            })
        }

        /// The collection's name from `opencollection.yml` (may be empty).
        pub fn name(&self) -> String {
            self.inner.borrow().workspace.collection().name.clone()
        }

        /// Whether the collection (with unsaved changes) has any script (wave 7, trust).
        #[wasm_bindgen(js_name = hasScripts)]
        pub fn has_scripts(&self) -> bool {
            rt_app::scripts::has_scripts(self.inner.borrow().workspace.collection())
        }

        pub fn tree(&self) -> Result<JsValue, JsValue> {
            to_js(&self.inner.borrow().workspace.tree())
        }

        pub fn request(&self, path: &str) -> Result<JsValue, JsValue> {
            to_js(
                &self
                    .inner
                    .borrow()
                    .workspace
                    .request(path)
                    .map_err(app_error)?,
            )
        }

        #[wasm_bindgen(js_name = createRequest)]
        pub fn create_request(&self, folder: &str, name: &str) -> Result<String, JsValue> {
            self.inner
                .borrow_mut()
                .workspace
                .create_request(folder, name)
                .map_err(app_error)
        }

        /// Makes sure folder `path` exists (segments by directory or display name), as an
        /// unsaved change; returns its path (wave 6).
        #[wasm_bindgen(js_name = createFolder)]
        pub fn create_folder(&self, path: &str) -> Result<String, JsValue> {
            self.inner
                .borrow_mut()
                .workspace
                .create_folder(path)
                .map_err(app_error)
        }

        /// What a plugin tab runs with (wave 6): its context and the URLs `{request}` covers,
        /// for `Plugins.renderTab`. `requestPath` `""`: a collection tab.
        #[wasm_bindgen(js_name = pluginScope)]
        pub fn plugin_scope(
            &self,
            #[wasm_bindgen(js_name = requestPath)] request_path: &str,
            environment: Option<String>,
        ) -> Result<JsValue, JsValue> {
            to_js(&rt_app::plugins::tab_scope(
                self.inner.borrow().workspace.collection(),
                request_path,
                environment.as_deref(),
            ))
        }

        #[wasm_bindgen(js_name = updateRequest)]
        pub fn update_request(&self, path: &str, spec: JsValue) -> Result<(), JsValue> {
            let spec = from_js(&spec, "request")?;
            self.inner
                .borrow_mut()
                .workspace
                .update_request(path, spec)
                .map_err(app_error)
        }

        pub fn dirty(&self) -> Result<JsValue, JsValue> {
            to_js(&self.inner.borrow().workspace.dirty())
        }

        /// The files to write, without changing this workspace (see the crate documentation).
        #[wasm_bindgen(js_name = prepareSave)]
        pub fn prepare_save(&self) -> Result<PreparedSave, JsValue> {
            let mut next = self.inner.borrow().workspace.clone();
            let changes = next.save().map_err(app_error)?;
            Ok(PreparedSave {
                changes: to_js(&changes)?,
                next: Some(next),
            })
        }

        /// Adopts a `prepareSave` result once its files are written.
        pub fn commit(&self, prepared: &mut PreparedSave) -> Result<(), JsValue> {
            let next = prepared
                .next
                .take()
                .ok_or_else(|| named_error("shell", "this save was already committed"))?;
            self.inner.borrow_mut().workspace = next;
            Ok(())
        }

        pub fn environments(&self) -> Result<JsValue, JsValue> {
            to_js(&self.inner.borrow().workspace.environments())
        }

        #[wasm_bindgen(js_name = updateEnvironment)]
        pub fn update_environment(&self, env: JsValue) -> Result<(), JsValue> {
            let env = from_js(&env, "environment")?;
            self.inner
                .borrow_mut()
                .workspace
                .update_environment(env)
                .map_err(app_error)
        }

        #[wasm_bindgen(js_name = deleteEnvironment)]
        pub fn delete_environment(&self, name: &str) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .workspace
                .delete_environment(name)
                .map_err(app_error)
        }

        pub fn preview(&self, path: &str, input: JsValue) -> Result<JsValue, JsValue> {
            let input: SendInput = from_js(&input, "input")?;
            let preview = self
                .inner
                .borrow()
                .workspace
                .preview(path, input)
                .map_err(app_error)?;
            to_js(&preview)
        }

        /// Resolves with the `StoredResponse`; transport failures are in `report.error`.
        /// `services`: plugins, token cache and prompts, cookie jar (wave 5,
        /// `services_js`), or `null`.
        pub fn send(
            &self,
            path: &str,
            input: JsValue,
            companion: JsValue,
            services: JsValue,
        ) -> Result<Promise, JsValue> {
            let input: SendInput = from_js(&input, "input")?;
            let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
                None
            } else {
                Some(from_js(&companion, "companion")?)
            };
            let snapshot = self
                .inner
                .borrow()
                .workspace
                .snapshot(path, input)
                .map_err(app_error)?;
            let parts = crate::services_js::SendServices::from_js(&services)?;
            let inner = Rc::clone(&self.inner);
            Ok(wasm_bindgen_futures::future_to_promise(async move {
                let options = SendOptions {
                    body_limit: BODY_LIMIT,
                    iteration: None,
                };
                let services = parts.services();
                let report = match companion {
                    Some(c) => {
                        let transport = CompanionTransport::new(c.url, c.token);
                        snapshot.send_with(&transport, &options, &services).await
                    }
                    None => {
                        snapshot
                            .send_with(&FetchTransport::new(), &options, &services)
                            .await
                    }
                };
                let mut report = report;
                parts.scripts.finish(&snapshot.collection, &mut report);
                let observe = snapshot.collection.config.observe.enabled;
                let mut stored = inner.borrow_mut().responses.insert(report);
                stored.observe = observe;
                to_js(&stored)
            }))
        }

        /// "Get token" (wave 5, strand C, `auth_js`): runs the OAuth 2.0 flow of the request
        /// at `path` now and stores the token through `services.auth`. Resolves with the
        /// `TokenInfo` (no value); rejects with an `AppError` (kind `auth`).
        #[wasm_bindgen(js_name = fetchToken)]
        pub fn fetch_token(
            &self,
            path: &str,
            input: JsValue,
            companion: JsValue,
            services: JsValue,
        ) -> Result<Promise, JsValue> {
            let input: SendInput = from_js(&input, "input")?;
            let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
                None
            } else {
                Some(from_js(&companion, "companion")?)
            };
            let snapshot = self
                .inner
                .borrow()
                .workspace
                .snapshot(path, input)
                .map_err(app_error)?;
            let auth = crate::auth_js::AuthSend::from_js(&services)?;
            Ok(wasm_bindgen_futures::future_to_promise(async move {
                let services = auth.services();
                let info = match companion {
                    Some(c) => {
                        let transport = CompanionTransport::new(c.url, c.token);
                        rt_app::auth::fetch_token(&snapshot, &transport, &services).await
                    }
                    None => {
                        rt_app::auth::fetch_token(&snapshot, &FetchTransport::new(), &services)
                            .await
                    }
                };
                to_js(&info.map_err(app_error)?)
            }))
        }

        #[wasm_bindgen(js_name = responseLines)]
        pub fn response_lines(&self, id: f64, start: f64, count: u32) -> Result<JsValue, JsValue> {
            let slice = self
                .inner
                .borrow()
                .responses
                .lines(int(id)?, int(start)?, count)
                .map_err(app_error)?;
            to_js(&slice)
        }

        #[wasm_bindgen(js_name = responseBytes)]
        pub fn response_bytes(
            &self,
            id: f64,
            offset: f64,
            length: u32,
        ) -> Result<Uint8Array, JsValue> {
            let bytes = self
                .inner
                .borrow()
                .responses
                .bytes(int(id)?, int(offset)?, length)
                .map_err(app_error)?;
            Ok(Uint8Array::from(&bytes[..]))
        }

        #[wasm_bindgen(js_name = filterJson)]
        pub fn filter_json(&self, id: f64, expression: &str) -> Result<String, JsValue> {
            self.inner
                .borrow()
                .responses
                .filter_json(int(id)?, expression)
                .map_err(app_error)
        }

        #[wasm_bindgen(js_name = releaseResponse)]
        pub fn release_response(&self, id: f64) -> Result<(), JsValue> {
            self.inner.borrow_mut().responses.release(int(id)?);
            Ok(())
        }

        /// `SearchHit[]` (`rt_app::search`, owner: wave 4, strand E).
        pub fn search(&self, query: &str, limit: u32) -> Result<JsValue, JsValue> {
            to_js(&rt_app::search::search(
                &self.inner.borrow().workspace,
                query,
                limit as usize,
            ))
        }
    }

    /// Result of `Workspace.prepareSave`.
    #[wasm_bindgen]
    pub struct PreparedSave {
        changes: JsValue,
        next: Option<Workspace>,
    }

    #[wasm_bindgen]
    impl PreparedSave {
        /// `FileChange[]` to write.
        #[wasm_bindgen(getter)]
        pub fn changes(&self) -> JsValue {
            self.changes.clone()
        }
    }
}

// Wave 4, one module per strand: git (A2), import (B), history (E).
// Wave 8: one module per strand.
#[cfg(target_arch = "wasm32")]
mod agents_js;
#[cfg(target_arch = "wasm32")]
mod ai_js;
#[cfg(target_arch = "wasm32")]
mod auth_js;
#[cfg(target_arch = "wasm32")]
mod companions_js;
// Wave 14, strand A: several collections in the sidebar.
mod access_js;
#[cfg(target_arch = "wasm32")]
mod catalogue_js;
#[cfg(target_arch = "wasm32")]
mod collections_js;
mod consumers_js;
#[cfg(target_arch = "wasm32")]
mod cookies_js;
mod debugger_js;
mod egress_js;
#[cfg(target_arch = "wasm32")]
mod envcompare_js;
#[cfg(target_arch = "wasm32")]
mod folder_scripts_js;
#[cfg(target_arch = "wasm32")]
mod git_js;
#[cfg(target_arch = "wasm32")]
mod grpc_js;
#[cfg(target_arch = "wasm32")]
mod guard_js;
#[cfg(target_arch = "wasm32")]
mod history_js;
#[cfg(target_arch = "wasm32")]
mod import_js;
#[cfg(target_arch = "wasm32")]
mod inbox_js;
#[cfg(target_arch = "wasm32")]
mod inspect_js;
#[cfg(target_arch = "wasm32")]
mod journal_js;
#[cfg(target_arch = "wasm32")]
mod lint_js;
#[cfg(target_arch = "wasm32")]
mod mcp_js;
mod notebook_js;
#[cfg(target_arch = "wasm32")]
mod paging_js;
#[cfg(target_arch = "wasm32")]
mod plugins_js;
#[cfg(target_arch = "wasm32")]
mod runner_js;
#[cfg(target_arch = "wasm32")]
mod scripts_js;
mod security_js;
mod sequence_js;
#[cfg(target_arch = "wasm32")]
mod services_js;
#[cfg(target_arch = "wasm32")]
mod share_js;
#[cfg(target_arch = "wasm32")]
mod shrink_js;
#[cfg(target_arch = "wasm32")]
mod snapshots_js;
#[cfg(target_arch = "wasm32")]
mod streams_js;
/// Wave 14: variable completion.
#[cfg(target_arch = "wasm32")]
mod variables_js;
#[cfg(target_arch = "wasm32")]
mod views_js;
// Wave 12 (Git for APIs): one module per strand.
#[cfg(target_arch = "wasm32")]
mod compare_js;
#[cfg(target_arch = "wasm32")]
mod contract_js;
#[cfg(target_arch = "wasm32")]
mod diffs_js;
#[cfg(target_arch = "wasm32")]
mod merges_js;
/// OpenAPI import wizard and run wizard.
#[cfg(target_arch = "wasm32")]
mod openapi_js;
#[cfg(target_arch = "wasm32")]
mod rescue_js;
#[cfg(target_arch = "wasm32")]
mod review_js;
#[cfg(target_arch = "wasm32")]
mod timeline_js;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_from_json() {
        let o = Options::from_json(
            r#"{"environment":"Local","vars":{"a":"1"},"globals":{"g":"2"},"bodyLimit":10}"#,
        )
        .unwrap();
        assert_eq!(o.environment.as_deref(), Some("Local"));
        assert_eq!(o.vars["a"], "1");
        assert_eq!(o.globals["g"], "2");
        assert_eq!(o.body_limit, Some(10));
        assert_eq!(Options::from_json("{}").unwrap(), Options::default());
        assert!(Options::from_json(r#"{"env":"x"}"#).is_err());
        assert!(Options::from_json(r#"{"vars":{"a":1}}"#).is_err());
    }

    #[test]
    fn app_errors_have_a_kind_and_a_readable_message() {
        use rt_app::AppError;
        assert_eq!(
            error_parts(&AppError::RequestNotFound("a.yml".into())),
            (
                "request-not-found".into(),
                "request not found: a.yml".into()
            )
        );
        assert_eq!(
            error_parts(&AppError::ResponseNotFound(7)),
            ("response-not-found".into(), "response not found: 7".into())
        );
        assert_eq!(
            error_parts(&AppError::Shell("disk full".into())),
            ("shell".into(), "disk full".into())
        );
    }

    #[test]
    fn companion_credentials() {
        let c: Companion =
            serde_json::from_str(r#"{"url":"http://127.0.0.1:47812","token":"t"}"#).unwrap();
        assert_eq!(c.url, "http://127.0.0.1:47812");
        assert_eq!(c.token, "t");
        assert!(serde_json::from_str::<Companion>(r#"{"url":"x"}"#).is_err());
    }

    #[test]
    fn mem_files_stay_inside_the_set() {
        let mut set = FileSet::default();
        set.files.insert("files/a.txt".into(), b"a".to_vec());
        let f = MemFiles(&set);
        assert_eq!(f.read("files/a.txt").unwrap(), b"a");
        assert_eq!(f.read("./files//a.txt").unwrap(), b"a");
        assert_eq!(f.read("files\\a.txt").unwrap(), b"a");
        for bad in [
            "/files/a.txt",
            "../a.txt",
            "files/../files/a.txt",
            "C:/a.txt",
            "b.txt",
        ] {
            assert!(f.read(bad).is_err(), "{bad}");
        }
    }
}
