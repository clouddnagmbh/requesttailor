// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The public plugin API, version [`API_VERSION`]: the manifest a plugin ships, and the JSON
//! messages host and plugin exchange. `API.md` describes the binary interface (exports,
//! imports, memory) and the rules; this module is its exact data definition.
//!
//! Public and versioned from wave 5 on: every change needs an entry in `CHANGELOG.md`.
//! Additive changes (new optional fields, new hooks, new blocks) keep the version; anything
//! else bumps it, and the host keeps running plugins built for older versions.
//!
//! All JSON is snake_case. Unknown fields are ignored by both sides, so a newer plugin can run
//! on an older host as long as it declares the older `api`.

use serde::{Deserialize, Serialize};

use rt_core::model::RequestSpec;
use rt_core::services::NoteLevel;
use rt_format::import::LossReport;

/// The API version this host implements. A plugin declares the version it was built for in
/// [`Manifest::api`]; the host runs plugins with `1 <= api <= API_VERSION`.
pub const API_VERSION: u32 = 1;

/// Name of the WebAssembly custom section that holds the manifest as UTF-8 JSON. A plugin is
/// a single `.wasm` file; there is no archive format.
pub const MANIFEST_SECTION: &str = "rt-plugin";

/// What a plugin is and what it asks for. Shown to the user before installing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Unique, `[a-z0-9][a-z0-9-]*`, at most 64 characters, e.g. `odata`.
    pub id: String,
    pub name: String,
    /// Semantic version of the plugin, e.g. `1.2.0`.
    pub version: String,
    /// Plugin API version the plugin was built for.
    pub api: u32,
    #[serde(default)]
    pub description: String,
    /// SPDX expression, e.g. `Apache-2.0`.
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub contributes: Contributions,
}

/// Rights a plugin requests. A plugin gets nothing it did not request, and only what the
/// user granted at install time (all or nothing in API version 1).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Permissions {
    /// Hosts the plugin may send requests to with the `http` host call: exact host names,
    /// `*.example.com` for subdomains, `{request}` for the host of the request the hook runs
    /// for, or `*` for any. Empty: no network at all.
    #[serde(default)]
    pub network: Vec<String>,
    /// A private key-value store (per plugin, per device).
    #[serde(default)]
    pub storage: bool,
    /// Read the non-secret variables in scope. Secret values are never available to plugins.
    #[serde(default)]
    pub variables: bool,
}

/// The extension points a plugin implements.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Contributions {
    /// Called before every send ([`Call::Prepare`]).
    #[serde(default)]
    pub prepare: bool,
    /// Called after every response ([`Call::Response`]).
    #[serde(default)]
    pub response: bool,
    #[serde(default)]
    pub tabs: Vec<TabContribution>,
    #[serde(default)]
    pub importers: Vec<ImporterContribution>,
    /// Offered on the app's first start ("What do you want to test?"). Only bundled plugins
    /// are offered; choosing it enables the plugin. Added in wave 6.
    #[serde(default)]
    pub welcome: Option<WelcomeChoice>,
}

/// A first-start choice of a bundled plugin (wave 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeChoice {
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabContribution {
    /// Unique within the plugin.
    pub id: String,
    pub title: String,
    pub place: TabPlace,
    /// Show the tab only for responses whose `Content-Type` starts with one of these
    /// (lowercase, e.g. `application/json`). Empty: always.
    #[serde(default)]
    pub content_types: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabPlace {
    /// Next to Params, Headers, Body, ... of the request editor.
    Request,
    /// Next to Body, Headers, ... of the response view.
    Response,
    /// A panel of the collection (next to the request tree), not tied to one request. Its
    /// [`TabTarget::request_path`] is `""` and the `spec` of the call is the default request.
    /// Added in wave 6.
    Collection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterContribution {
    /// Unique within the plugin.
    pub id: String,
    /// Shown in the import dialog, e.g. "SAP API Business Hub".
    pub label: String,
    /// File name extensions without the dot, lowercase, e.g. `["edmx", "xml"]`.
    #[serde(default)]
    pub extensions: Vec<String>,
}

// -------------------------------------------------------------------------------------------
// Data handed to plugins. Secret values are replaced by `***` before anything reaches a
// plugin; a plugin cannot tell a masked value from a typed `***`.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<PluginBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Final URL after redirects.
    pub url: String,
    pub body: PluginBody,
    /// The body was cut at the host's limit (see `API.md`).
    pub truncated: bool,
}

/// A body: UTF-8 text as `text`, anything else as standard base64 in `base64`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PluginBody {
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub base64: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContext {
    /// Path of the request file, relative to the collection root.
    pub request_path: String,
    pub environment: Option<String>,
    /// Non-secret variables in scope; empty without the `variables` permission.
    #[serde(default)]
    pub variables: Vec<(String, String)>,
}

/// What a tab is shown for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTarget {
    pub request_path: String,
    /// For response tabs: the response in the host's response store.
    pub response_id: Option<u64>,
}

/// A file for an importer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginFile {
    pub name: String,
    /// Standard base64.
    pub base64: String,
}

// -------------------------------------------------------------------------------------------
// Host → plugin: the argument of `rt_call`.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "hook", rename_all = "kebab-case")]
pub enum Call {
    /// Reply: [`PrepareReply`].
    Prepare {
        request: PluginRequest,
        context: PluginContext,
    },
    /// Reply: [`NotesReply`].
    Response {
        request: PluginRequest,
        response: PluginResponse,
        context: PluginContext,
    },
    /// Reply: [`ViewReply`]. `spec` is the request as edited (unsaved changes included,
    /// templates unresolved, secret values never present: the files hold none).
    RenderTab {
        tab: String,
        target: TabTarget,
        spec: RequestSpec,
        response: Option<PluginResponse>,
        /// What the last [`ViewReply::state`] of this tab was, `null` the first time.
        state: Option<serde_json::Value>,
        /// Selected environment and its non-secret variables (variables only with the
        /// `variables` permission). Added in wave 6; older hosts send none.
        #[serde(default)]
        context: Option<PluginContext>,
    },
    /// The user clicked an action or submitted a form of the tab. Reply: [`ViewReply`].
    Action {
        tab: String,
        action: String,
        target: TabTarget,
        spec: RequestSpec,
        response: Option<PluginResponse>,
        state: Option<serde_json::Value>,
        /// Form values by field name (forms only).
        #[serde(default)]
        input: Vec<(String, String)>,
        /// As for [`Call::RenderTab`]. Added in wave 6.
        #[serde(default)]
        context: Option<PluginContext>,
    },
    /// Reply: [`DetectReply`].
    Detect {
        importer: String,
        files: Vec<PluginFile>,
    },
    /// Reply: [`ImportReply`].
    Import {
        importer: String,
        files: Vec<PluginFile>,
    },
}

/// What `rt_call` returns: exactly one of `ok` (the hook's reply) or `error` (a message the
/// host shows; for `prepare` it stops the send).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply<T> {
    #[serde(default)]
    pub ok: Option<T>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PrepareReply {
    #[serde(default)]
    pub patch: RequestPatch,
    #[serde(default)]
    pub notes: Vec<PluginNote>,
}

/// Changes to the outgoing request. Applied in this order: `url`, `remove_headers`,
/// `set_headers`, `body`. A value that contains `***` is refused (it would send the mask).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RequestPatch {
    #[serde(default)]
    pub url: Option<String>,
    /// Header names, case-insensitive.
    #[serde(default)]
    pub remove_headers: Vec<String>,
    /// Replaces every header of that name (case-insensitive), or adds it.
    #[serde(default)]
    pub set_headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<PluginBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotesReply {
    #[serde(default)]
    pub notes: Vec<PluginNote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginNote {
    pub level: NoteLevel,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ViewReply {
    pub view: View,
    /// Kept by the host and passed back with the next call for this tab.
    #[serde(default)]
    pub state: Option<serde_json::Value>,
    /// Replaces the request in the editor (an unsaved change, like typing). Request tabs only.
    #[serde(default)]
    pub update_request: Option<RequestSpec>,
    /// New requests for the collection (unsaved, like "New request"), e.g. from an entity
    /// browser. Names that exist get a suffix (API.md). Added in wave 6.
    #[serde(default)]
    pub create_requests: Vec<NewRequest>,
    /// New environments (unsaved), e.g. a DEV/QAS/PRD template. Secret variables are
    /// created without a value (the user sets it); a secret with a value is refused. An
    /// existing environment keeps its variables and gets only the missing ones (API.md).
    /// Added in wave 6.
    #[serde(default)]
    pub add_environments: Vec<PluginEnvironment>,
}

/// A request a view adds to the collection (wave 6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewRequest {
    /// Folder path relative to the collection root, `""` for the root; created if missing.
    #[serde(default)]
    pub folder: String,
    pub name: String,
    pub spec: RequestSpec,
}

/// An environment a view adds to the collection (wave 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEnvironment {
    pub name: String,
    #[serde(default)]
    pub variables: Vec<PluginVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginVariable {
    pub name: String,
    #[serde(default)]
    pub value: String,
    /// A secret variable without a value: the user sets it in Environments (keychain).
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DetectReply {
    pub matches: bool,
}

/// A converted collection, in the same shape as the built-in importers' result: OpenCollection
/// files (path → text) and the loss report. The host validates the files with `rt-format`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReply {
    pub collection_name: String,
    pub files: Vec<(String, String)>,
    pub report: LossReport,
}

// -------------------------------------------------------------------------------------------
// Views: declarative UI for tabs. The host renders them with its own components; plugins
// never run code in the UI.

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct View {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Block {
    Heading {
        text: String,
    },
    /// Plain text; no markup is interpreted.
    Text {
        text: String,
    },
    Notice {
        level: NoteLevel,
        text: String,
    },
    KeyValue {
        rows: Vec<(String, String)>,
    },
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Code {
        #[serde(default)]
        language: Option<String>,
        text: String,
    },
    Tree {
        nodes: Vec<TreeItem>,
    },
    Form {
        id: String,
        fields: Vec<FormField>,
        submit: String,
    },
    Actions {
        actions: Vec<ActionButton>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeItem {
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub children: Vec<TreeItem>,
    /// Clicking the item runs this action.
    #[serde(default)]
    pub action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormField {
    pub name: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default)]
    pub value: String,
    /// For `select`.
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Select,
    Checkbox,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionButton {
    /// Passed as `action` in [`Call::Action`].
    pub id: String,
    pub label: String,
}

// -------------------------------------------------------------------------------------------
// Plugin → host: the argument of the `rt.host` import.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum HostCall {
    /// Always allowed. Reply: `null`.
    Log { level: NoteLevel, message: String },
    /// Needs `permissions.network` to cover the URL's host. Reply: [`PluginResponse`].
    /// `credentials: true` applies the auth of the request the current hook runs for
    /// (prepare and response hooks only); the plugin never sees those values.
    Http {
        request: PluginRequest,
        #[serde(default)]
        credentials: bool,
        /// Send and store cookies in the cookie jar of the current send's environment, so a
        /// handshake (e.g. a CSRF token bound to a session cookie) shares the session with the
        /// request. Only for the request's origin, in the prepare and response hooks. Added in
        /// wave 6.
        #[serde(default)]
        session: bool,
    },
    /// Needs `permissions.storage`. Reply: the value or `null`.
    StorageGet { key: String },
    /// Needs `permissions.storage`. Reply: `null`.
    StorageSet {
        key: String,
        value: serde_json::Value,
    },
    /// Needs `permissions.storage`. Reply: `null`.
    StorageDelete { key: String },
    /// Always allowed. Reply: Unix milliseconds.
    Now,
}
