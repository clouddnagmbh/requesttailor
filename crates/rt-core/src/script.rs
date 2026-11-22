// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What a script runner gets and gives back (wave 7). The core decides *when* scripts run
//! (`send_with`); `rt-script` decides *how* (QuickJS sandbox, behind
//! [`crate::services::ScriptRunner`]). The script-facing API is `crates/rt-script/api.d.ts`.
//!
//! Order in `send_with`:
//! 1. `before-request` scripts (collection, folders, request; outermost first) on the request
//!    as written (templates unresolved). They may set variables (runtime layer of this send)
//!    and change method, URL, headers and a text body; then `prepare` resolves.
//! 2. … auth, plugins, signing, cookies, the wire …
//! 3. `after-response` scripts, then `tests` scripts (same order), then declarative
//!    assertions and variable actions ([`crate::assert`]).
//!
//! Variable changes are reported ([`crate::send::SendReport::var_changes`]); keeping them
//! (session in the app, the run in the runner and CLI) is the caller's job. Files are never
//! written by scripts.
//!
//! Secrets: scripts see secret values (a pre-request script may sign with a key). Everything
//! that leaves a script (logs, test names and messages, variable values) is masked in the
//! report like any other secret. Shells run scripts only for collections the user trusts.

use serde::{Deserialize, Serialize};

use crate::model::{Body, KeyValue, RawKind, RequestSpec, ScriptPhase, VarScope};
use crate::services::LocalBoxFuture;
use crate::vars::{Scope, VarSource};

/// The request as a script sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    /// Enabled headers, in order.
    pub headers: Vec<(String, String)>,
    /// Text bodies only; `None` for no body or a body a script cannot edit (form, multipart,
    /// file, binary), which a script then leaves as it is.
    pub body: Option<String>,
}

/// The response as a script sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScriptResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// The body as received (at most the report's body limit).
    pub body: Vec<u8>,
    /// Milliseconds from sending the request until the last body byte.
    pub time_ms: u64,
    /// URL after redirects.
    pub url: String,
}

/// A variable in scope, highest layer last (as [`Scope::layers`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptVar {
    pub name: String,
    pub value: String,
    pub source: VarSource,
    pub secret: bool,
}

/// One script with where it comes from (for logs and errors).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptSource {
    /// `collection`, `folder:<path>` or `request`.
    pub origin: String,
    pub code: String,
}

/// The runner's iteration (wave 7, strand D); `None` for a single send.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Iteration {
    /// 0-based.
    pub index: u32,
    pub count: u32,
    /// The data row of this iteration (CSV columns as strings, JSON values as they are).
    pub data: Vec<(String, serde_json::Value)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub request_path: String,
    pub request_name: String,
    pub collection_name: String,
    pub environment: Option<String>,
    pub iteration: Option<Iteration>,
}

/// One call of the runner: all scripts of one phase for one request.
pub struct ScriptCall<'a> {
    pub phase: ScriptPhase,
    pub scripts: Vec<ScriptSource>,
    pub request: ScriptRequest,
    /// `AfterResponse` and `Tests` only.
    pub response: Option<ScriptResponse>,
    pub variables: Vec<ScriptVar>,
    pub info: ScriptInfo,
    /// For `rt.sendRequest` (the "request API": the only network access a script has).
    pub sender: Option<&'a dyn ScriptSender>,
}

/// Sends a request on behalf of a script, through the same transport (no scripts, no
/// plugins, no auth inheritance: what the script builds is what goes out).
pub trait ScriptSender {
    fn send<'a>(
        &'a self,
        request: ScriptRequest,
    ) -> LocalBoxFuture<'a, Result<ScriptResponse, String>>;
}

/// What the runner gives back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptOutcome {
    /// `BeforeRequest`: the changed request (`None`: unchanged).
    pub request: Option<ScriptRequest>,
    pub var_changes: Vec<VarChange>,
    pub tests: Vec<TestResult>,
    pub logs: Vec<ScriptLog>,
    pub flow: Flow,
    /// The script failed (exception, time or memory limit). `BeforeRequest`: the send stops;
    /// later phases: reported, the response stays.
    pub error: Option<String>,
}

/// A variable a script or action set (`value: None`: unset).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarChange {
    pub name: String,
    pub value: Option<String>,
    pub scope: VarScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    /// The assertion message of a failed test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// [`ScriptSource::origin`].
    pub origin: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Log,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLog {
    pub level: LogLevel,
    pub text: String,
    pub origin: String,
}

/// Runner control from scripts (`rt.runner.skip()`, `rt.runner.setNext(...)`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Flow {
    #[default]
    Continue,
    /// `BeforeRequest` only: do not send this request.
    Skip,
    /// Continue the run with this request (name or path); `None`: stop the run.
    Next { request: Option<String> },
}

/// Result of one declarative assertion ([`crate::model::Assertion`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertionResult {
    pub expression: String,
    pub operator: String,
    /// Expected value after variable substitution.
    pub expected: String,
    /// What the expression evaluated to, as text; `None` if it did not evaluate.
    pub actual: Option<String>,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The name of a phase as in OpenCollection (`before-request`, …).
pub fn phase_name(phase: ScriptPhase) -> &'static str {
    match phase {
        ScriptPhase::BeforeRequest => "before-request",
        ScriptPhase::AfterResponse => "after-response",
        ScriptPhase::Tests => "tests",
    }
}

/// The scripts of `phase` that apply to a request, outermost first.
pub fn scripts_for(
    context: &crate::model::RequestContext<'_>,
    phase: ScriptPhase,
) -> Vec<ScriptSource> {
    let collection = context
        .collection
        .defaults
        .scripts
        .iter()
        .map(|s| (String::from("collection"), s));
    let folders = context.folders.iter().flat_map(|f| {
        f.defaults
            .scripts
            .iter()
            .map(move |s| (format!("folder:{}", f.path), s))
    });
    let request = context
        .request
        .spec
        .scripts
        .iter()
        .map(|s| (String::from("request"), s));
    collection
        .chain(folders)
        .chain(request)
        .filter(|(_, s)| s.phase == phase && !s.code.trim().is_empty())
        .map(|(origin, s)| ScriptSource {
            origin,
            code: s.code.clone(),
        })
        .collect()
}

/// Every variable in `scope`, lowest layer first.
pub fn variables_of(scope: &Scope) -> Vec<ScriptVar> {
    scope
        .layers
        .iter()
        .flat_map(|layer| {
            layer.vars.iter().map(|(name, value, secret)| ScriptVar {
                name: name.clone(),
                value: value.clone(),
                source: layer.source.clone(),
                secret: *secret,
            })
        })
        .collect()
}

/// The request of `spec` as a `before-request` script sees it: as written, templates
/// unresolved, the request's own enabled headers.
pub fn request_of(spec: &RequestSpec) -> ScriptRequest {
    ScriptRequest {
        method: spec.method.clone(),
        url: spec.url.clone(),
        headers: spec
            .headers
            .iter()
            .filter(|h| h.enabled)
            .map(|h| (h.name.clone(), h.value.clone()))
            .collect(),
        body: match &spec.body {
            Body::Raw { data, .. } => Some(data.clone()),
            _ => None,
        },
    }
}

/// Applies a script's changes to `spec`. Headers: the script's list replaces the enabled
/// headers (disabled ones stay). Body: a text body keeps its kind; a script that sets a body
/// on a request without one makes it raw text (JSON if it parses as JSON).
pub fn apply_request(spec: &mut RequestSpec, changed: &ScriptRequest) {
    spec.method.clone_from(&changed.method);
    spec.url.clone_from(&changed.url);
    let disabled = spec.headers.iter().filter(|h| !h.enabled).cloned();
    spec.headers = changed
        .headers
        .iter()
        .map(|(name, value)| KeyValue {
            name: name.clone(),
            value: value.clone(),
            enabled: true,
        })
        .chain(disabled)
        .collect();
    if let Some(text) = &changed.body {
        spec.body = match &spec.body {
            Body::Raw { kind, .. } => Body::Raw {
                kind: *kind,
                data: text.clone(),
            },
            Body::None => Body::Raw {
                kind: if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                    RawKind::Json
                } else {
                    RawKind::Text
                },
                data: text.clone(),
            },
            // Form, multipart and file bodies are not editable from scripts.
            other => other.clone(),
        };
    }
}
