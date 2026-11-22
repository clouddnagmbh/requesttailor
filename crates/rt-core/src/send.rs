// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One entry point for "send this request": CLI, WASM and later the app all call this, so
//! their results are identical (product principle 5). `SendReport` is also the JSON that
//! `rt send` prints and that `rt-wasm` returns.
//!
//! Owner: wave 2, strand D.

use serde::{Deserialize, Serialize};

use crate::model::{Auth, Body, Collection, RequestContext, ScriptPhase, Variable};
use crate::prepare::{FileSource, PrepareError, PrepareInput, Prepared, prepare};
use crate::script::{
    AssertionResult, Flow, Iteration, ScriptCall, ScriptInfo, ScriptLog, ScriptOutcome,
    ScriptRequest, ScriptResponse, ScriptSender, TestResult, VarChange,
};
use crate::services::{HookContext, LocalBoxFuture, Note, NoteLevel, Services};
use crate::transport::{
    CompletedResponse, HttpRequest, HttpVersion, Redirect, Timing, Transport, TransportError,
    collect,
};
use crate::vars::VarUse;
use crate::vars::{Scope, VarLayer, VarSource};

/// Version of the `SendReport` JSON shape. Bump on breaking changes.
pub const REPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendOptions {
    /// Keep at most this many body bytes in the report.
    pub body_limit: usize,
    /// The runner's iteration, passed to scripts (wave 7). `None` for a single send.
    #[serde(default)]
    pub iteration: Option<Iteration>,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            body_limit: 10 * 1024 * 1024,
            iteration: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentRequest {
    pub method: String,
    pub url: String,
    /// Secret values are masked as `***`.
    pub headers: Vec<(String, String)>,
    pub body_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding", content = "data", rename_all = "lowercase")]
pub enum ReportBody {
    /// Valid UTF-8.
    Utf8(String),
    /// Anything else, standard base64.
    Base64(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedResponse {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub redirects: Vec<Redirect>,
    pub body: ReportBody,
    pub size: u64,
    pub truncated: bool,
    pub timing: Timing,
    #[serde(with = "crate::serde_ms")]
    pub total: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum SendError {
    /// Request path not in the collection.
    NotFound(String),
    /// Variable, auth or body preparation failed.
    Prepare(String),
    /// Getting or applying credentials failed at send time (OAuth 2.0 token, SigV4 signing,
    /// client certificate). Wave 5.
    Auth(String),
    /// A plugin hook stopped the send. Wave 5.
    Plugin(String),
    /// A `before-request` script failed (exception, time or memory limit). Wave 7.
    Script(String),
    Transport(TransportError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendReport {
    pub version: u32,
    /// Request path relative to the collection root.
    pub request_path: String,
    pub environment: Option<String>,
    /// What went on the wire; `None` if preparation failed.
    pub request: Option<SentRequest>,
    pub response: Option<ReceivedResponse>,
    pub variables: Vec<VarUse>,
    pub unresolved: Vec<String>,
    pub error: Option<SendError>,
    /// Messages from the auth step and plugin hooks (wave 5). Omitted from the JSON when
    /// empty, so wave 2 reports are unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// `test(...)` results of the `tests` scripts (wave 7). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<TestResult>,
    /// Declarative assertions (wave 7). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<AssertionResult>,
    /// `console.*` output of scripts, secrets masked (wave 7). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<ScriptLog>,
    /// Variables set by scripts and actions, in order; secret values masked (wave 7). The
    /// caller keeps them (session, run). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub var_changes: Vec<VarChange>,
    /// Runner control requested by a script (wave 7); `None`: continue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<Flow>,
}

/// What `rt send` prints in place of a secret value.
pub const MASK: &str = "***";

/// Finds the request, prepares it, sends it and reads the response.
///
/// Never fails: every outcome, including "request not found", is a [`SendReport`] with
/// `error` set. Secret values are masked in `report.request`; the response is reported as
/// received.
///
/// Without services: no token cache, no prompts, no cookies, no plugins. See [`send_with`].
pub async fn send<T: Transport>(
    collection: &Collection,
    request_path: &str,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    transport: &T,
    options: &SendOptions,
) -> SendReport {
    send_with(
        collection,
        request_path,
        input,
        files,
        transport,
        options,
        &Services::none(),
    )
    .await
}

/// [`send`] with the shell's services (wave 5). The pipeline after `prepare`, in order:
///
/// 1. OAuth 2.0 token: cached, refreshed or fetched (`auth::oauth2`, strand C).
/// 2. Plugin `before_send` hooks, on the masked request (`services.hooks`, strand A).
/// 3. AWS SigV4 signature over the final request (`auth::sigv4`, strand D).
/// 4. Client certificate for the host (`tls::client_identity`, strand D).
/// 5. Cookies from the environment's jar (`cookies`, strand D).
/// 6. The wire; then `Set-Cookie` into the jar and the plugin `after_response` hooks.
///
/// Every value that step 1 or 3 adds (tokens, signatures' secret inputs) is masked in the
/// report like a secret variable.
pub async fn send_with<T: Transport>(
    collection: &Collection,
    request_path: &str,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
) -> SendReport {
    let report = SendReport {
        version: REPORT_VERSION,
        request_path: request_path.to_owned(),
        environment: input.environment.map(str::to_owned),
        request: None,
        response: None,
        variables: Vec::new(),
        unresolved: Vec::new(),
        error: None,
        notes: Vec::new(),
        tests: Vec::new(),
        assertions: Vec::new(),
        logs: Vec::new(),
        var_changes: Vec::new(),
        flow: None,
    };
    let Some(context) = collection.request(request_path) else {
        return SendReport {
            error: Some(SendError::NotFound(request_path.to_owned())),
            ..report
        };
    };
    let mut report = report;
    let mut runtime = input.runtime.clone();
    let mut changed = None;
    // Wave 7: `before-request` scripts, on the request as written.
    if let Some(runner) = services.scripts {
        let scripts = crate::script::scripts_for(&context, ScriptPhase::BeforeRequest);
        if !scripts.is_empty() {
            let scope = crate::prepare::scope(&context, input).unwrap_or_default();
            let mut secrets = declared_secret_values(&context, input);
            add_secrets(&mut secrets, scope_secrets(&scope));
            let sender = TransportSender {
                transport,
                body_limit: options.body_limit,
            };
            let outcome = runner
                .run(ScriptCall {
                    phase: ScriptPhase::BeforeRequest,
                    scripts,
                    request: crate::script::request_of(&context.request.spec),
                    response: None,
                    variables: crate::script::variables_of(&scope),
                    info: script_info(&context, input, options),
                    sender: Some(&sender),
                })
                .await;
            let request = outcome.request.clone();
            let error = outcome.error.clone();
            let flow = outcome.flow.clone();
            for change in &outcome.var_changes {
                set_runtime(&mut runtime, change);
            }
            record(&mut report, outcome, &secrets);
            if let Some(e) = error {
                report.error = Some(SendError::Script(mask(&e, &secrets)));
                return report;
            }
            if flow == Flow::Skip {
                report.flow = Some(Flow::Skip);
                return report;
            }
            if let Some(request) = request {
                let mut entry = context.request.clone();
                crate::script::apply_request(&mut entry.spec, &request);
                changed = Some(entry);
            }
        }
    }
    let input = PrepareInput {
        environment: input.environment,
        globals: input.globals.clone(),
        runtime,
    };
    let context = match &changed {
        Some(entry) => RequestContext {
            collection: context.collection,
            folders: context.folders.clone(),
            request: entry,
        },
        None => context,
    };
    let prepared = prepare(&context, &input, files);
    finish(
        report, &context, &input, files, prepared, transport, options, services,
    )
    .await
}

/// Runs `after-response` and `tests` scripts, then assertions and actions (wave 7). Variable
/// changes of earlier steps are visible to later ones (runtime layer).
#[allow(clippy::too_many_arguments)]
async fn after_response<T: Transport>(
    report: &mut SendReport,
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    scope: &Scope,
    request: ScriptRequest,
    response: ScriptResponse,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    secrets: &[String],
) {
    let mut scope = scope.clone();
    let sender = TransportSender {
        transport,
        body_limit: options.body_limit,
    };
    if let Some(runner) = services.scripts {
        for phase in [ScriptPhase::AfterResponse, ScriptPhase::Tests] {
            let scripts = crate::script::scripts_for(context, phase);
            if scripts.is_empty() {
                continue;
            }
            let outcome = runner
                .run(ScriptCall {
                    phase,
                    scripts,
                    request: request.clone(),
                    response: Some(response.clone()),
                    variables: crate::script::variables_of(&scope),
                    info: script_info(context, input, options),
                    sender: Some(&sender),
                })
                .await;
            for change in &outcome.var_changes {
                set_scope(&mut scope, change);
            }
            if let Some(e) = &outcome.error {
                report.notes.push(Note {
                    source: format!("script:{}", crate::script::phase_name(phase)),
                    level: NoteLevel::Error,
                    text: mask(e, secrets),
                });
            }
            record(report, outcome, secrets);
        }
    }
    let spec = &context.request.spec;
    report.assertions = crate::assert::evaluate(&spec.assertions, &response, &scope)
        .into_iter()
        .map(|mut a| {
            a.expected = mask(&a.expected, secrets);
            a.actual = a.actual.map(|v| mask(&v, secrets));
            a.error = a.error.map(|e| mask(&e, secrets));
            a
        })
        .collect();
    report.var_changes.extend(
        crate::assert::apply_actions(&spec.actions, &response)
            .into_iter()
            .map(|mut c| {
                c.value = c.value.map(|v| mask(&v, secrets));
                c
            }),
    );
}

/// Puts a variable change into the runtime layer of `scope` (added if missing).
fn set_scope(scope: &mut Scope, change: &VarChange) {
    let index = match scope
        .layers
        .iter()
        .position(|l| l.source == VarSource::Runtime)
    {
        Some(i) => i,
        None => {
            scope.layers.push(VarLayer {
                source: VarSource::Runtime,
                vars: Vec::new(),
            });
            scope.layers.len() - 1
        }
    };
    let layer = &mut scope.layers[index];
    layer.vars.retain(|(name, _, _)| name != &change.name);
    if let Some(value) = &change.value {
        layer.vars.push((change.name.clone(), value.clone(), false));
    }
}

/// Sends requests built by scripts (`rt.sendRequest`) through the transport of the send.
struct TransportSender<'t, T> {
    transport: &'t T,
    body_limit: usize,
}

impl<T: Transport> ScriptSender for TransportSender<'_, T> {
    fn send<'a>(
        &'a self,
        request: ScriptRequest,
    ) -> LocalBoxFuture<'a, Result<ScriptResponse, String>> {
        Box::pin(async move {
            let request = HttpRequest {
                method: request.method,
                url: request.url,
                headers: request.headers,
                body: request.body.map(|b| crate::transport::HttpBody {
                    content_type: None,
                    bytes: b.into_bytes().into(),
                }),
                options: crate::transport::SendOptions::default(),
            };
            let started = web_time::Instant::now();
            let response = self
                .transport
                .send(request)
                .await
                .map_err(|e| e.to_string())?;
            let completed = collect(response, self.body_limit, started)
                .await
                .map_err(|e| e.to_string())?;
            Ok(script_response(&completed))
        })
    }
}

fn script_response(c: &CompletedResponse) -> ScriptResponse {
    ScriptResponse {
        status: c.head.status,
        headers: c.head.headers.clone(),
        body: c.body.to_vec(),
        time_ms: u64::try_from(c.total.as_millis()).unwrap_or(u64::MAX),
        url: c.head.url.clone(),
    }
}

fn script_info(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    options: &SendOptions,
) -> ScriptInfo {
    ScriptInfo {
        request_path: context.request.path.clone(),
        request_name: context.request.spec.name.clone(),
        collection_name: context.collection.name.clone(),
        environment: input.environment.map(str::to_owned),
        iteration: options.iteration.clone(),
    }
}

/// Every secret value in `scope`.
fn scope_secrets(scope: &Scope) -> Vec<String> {
    scope
        .layers
        .iter()
        .flat_map(|l| l.vars.iter())
        .filter(|(_, _, secret)| *secret)
        .map(|(_, value, _)| value.clone())
        .collect()
}

/// Applies a variable change to the runtime layer of this send.
fn set_runtime(runtime: &mut Vec<(String, String)>, change: &VarChange) {
    runtime.retain(|(name, _)| name != &change.name);
    if let Some(value) = &change.value {
        runtime.push((change.name.clone(), value.clone()));
    }
}

/// Moves what a script produced into the report, masked.
fn record(report: &mut SendReport, outcome: ScriptOutcome, secrets: &[String]) {
    report.logs.extend(outcome.logs.into_iter().map(|mut l| {
        l.text = mask(&l.text, secrets);
        l
    }));
    report.tests.extend(outcome.tests.into_iter().map(|mut t| {
        t.name = mask(&t.name, secrets);
        t.error = t.error.map(|e| mask(&e, secrets));
        t
    }));
    report
        .var_changes
        .extend(outcome.var_changes.into_iter().map(|mut c| {
            c.value = c.value.map(|v| mask(&v, secrets));
            c
        }));
    if let flow @ Flow::Next { .. } = outcome.flow {
        report.flow = Some(flow);
    }
}

/// Everything after `prepare`. Split out so the orchestration can be tested with a
/// hand-built `Prepared`.
#[allow(clippy::too_many_arguments)]
async fn finish<T: Transport>(
    mut report: SendReport,
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    prepared: Result<Prepared, PrepareError>,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
) -> SendReport {
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            report.unresolved = unresolved_fallback(context, input);
            let secrets = declared_secret_values(context, input);
            report.error = Some(SendError::Prepare(mask(&e.to_string(), &secrets)));
            return report;
        }
    };
    let mut secrets = secret_values(context, input, &prepared);
    let Prepared {
        mut request,
        auth,
        scope,
        used,
        unresolved,
    } = prepared;
    report.unresolved = unresolved;
    let scope_name = input.environment.unwrap_or("");
    let hook_context = HookContext {
        request_path: report.request_path.clone(),
        environment: input.environment.map(str::to_owned),
        variables: used
            .iter()
            .filter(|u| !u.secret)
            .filter_map(|u| {
                let (value, _, secret) = scope.get(&u.name)?;
                (!secret).then(|| (u.name.clone(), value.to_owned()))
            })
            .collect(),
    };
    report.variables = used;
    let fail = |mut report: SendReport, error: SendError, secrets: &[String]| {
        report.error = Some(match error {
            SendError::Auth(m) => SendError::Auth(mask(&m, secrets)),
            SendError::Plugin(m) => SendError::Plugin(mask(&m, secrets)),
            e => e,
        });
        report
    };

    // 1. OAuth 2.0.
    if let Auth::OAuth2(oauth) = &auth {
        match crate::auth::oauth2::authorize(oauth, &scope, request, transport, services).await {
            Ok(authorized) => {
                request = authorized.request;
                add_secrets(&mut secrets, authorized.secrets);
                report.notes.extend(authorized.notes);
            }
            Err(e) => return fail(report, SendError::Auth(e), &secrets),
        }
    }
    // 2. Plugins see the request with secrets masked; `rt-plugin` does the masking.
    if let Some(hooks) = services.hooks {
        match hooks.before_send(request, &secrets, &hook_context).await {
            Ok((changed, notes)) => {
                request = changed;
                report.notes.extend(notes);
            }
            Err(e) => return fail(report, SendError::Plugin(e), &secrets),
        }
    }
    // 3. SigV4 signs what goes on the wire, so it comes after every change.
    if let Auth::AwsSigV4(aws) = &auth {
        match crate::auth::sigv4::sign(&mut request, aws, &scope, web_time::SystemTime::now()) {
            Ok(values) => add_secrets(&mut secrets, values),
            Err(e) => return fail(report, SendError::Auth(e), &secrets),
        }
    }
    // 4. Client certificate.
    match crate::tls::client_identity(context, input.environment, &request.url, &scope, files) {
        Ok(identity) => request.options.client_identity = identity,
        Err(e) => return fail(report, SendError::Auth(e), &secrets),
    }
    // 5. Cookies.
    if let Some(jar) = services.cookies
        && let Some(cookie) = jar.cookie_header(scope_name, &request.url).await
    {
        crate::cookies::add_cookie_header(&mut request.headers, &cookie);
    }

    report.request = Some(sent_request(&request, &secrets));
    let script_request = ScriptRequest {
        method: request.method.clone(),
        url: request.url.clone(),
        headers: request.headers.clone(),
        body: request
            .body
            .as_ref()
            .and_then(|b| String::from_utf8(b.bytes.to_vec()).ok()),
    };
    let started = web_time::Instant::now();
    let result = match transport.send(request).await {
        Ok(response) => collect(response, options.body_limit, started).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(completed) => {
            if let Some(jar) = services.cookies {
                let set_cookie = crate::cookies::set_cookie_values(&completed.head);
                if !set_cookie.is_empty() {
                    jar.store(scope_name, &completed.head.url, &set_cookie)
                        .await;
                }
            }
            if let Some(hooks) = services.hooks {
                let notes = hooks
                    .after_response(&completed.head, &completed.body, &hook_context)
                    .await;
                report.notes.extend(notes);
            }
            // Wave 7: after-response and tests scripts, assertions, actions.
            let response = script_response(&completed);
            after_response(
                &mut report,
                context,
                input,
                &scope,
                script_request,
                response,
                transport,
                options,
                services,
                &secrets,
            )
            .await;
            report.response = Some(received(completed));
        }
        // Transports quote the URL and header values in their messages; secrets must not
        // reach the report (CLI JSON, UI, agents) through an error either.
        Err(e) => report.error = Some(SendError::Transport(mask_transport_error(e, &secrets))),
    }
    for note in &mut report.notes {
        note.text = mask(&note.text, &secrets);
    }
    report
}

/// Adds values to mask, keeping the order `mask` relies on (longest first).
fn add_secrets(secrets: &mut Vec<String>, values: Vec<String>) {
    secrets.extend(values.into_iter().filter(|v| !v.is_empty()));
    secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    secrets.dedup();
}

/// `e` with every secret in its message replaced by [`MASK`].
fn mask_transport_error(e: TransportError, secrets: &[String]) -> TransportError {
    use TransportError::*;
    let m = |s: String| mask(&s, secrets);
    match e {
        InvalidUrl(s) => InvalidUrl(m(s)),
        Dns(s) => Dns(m(s)),
        Connect(s) => Connect(m(s)),
        Tls(s) => Tls(m(s)),
        Blocked(s) => Blocked(m(s)),
        Other(s) => Other(m(s)),
        e @ (Timeout | TooManyRedirects(_)) => e,
    }
}

/// Values of the secret variables that apply to this request, without a preparation (for
/// masking preparation errors): declared values and runtime/global values of secret names.
fn declared_secret_values(context: &RequestContext<'_>, input: &PrepareInput<'_>) -> Vec<String> {
    let secret: Vec<&Variable> = declared_vars(context, input).filter(|v| v.secret).collect();
    let mut values: Vec<String> = secret.iter().filter_map(|v| v.value.clone()).collect();
    for (name, value) in input.globals.iter().chain(&input.runtime) {
        if secret.iter().any(|v| &v.name == name) {
            values.push(value.clone());
        }
    }
    values.retain(|v| !v.is_empty());
    values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    values.dedup();
    values
}

/// The request as it goes on the wire, with secrets masked. The body's content type is listed
/// as a header when the request does not set one, because the transport adds it.
fn sent_request(request: &HttpRequest, secrets: &[String]) -> SentRequest {
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(name, value)| {
            // Cookies are session secrets (wave 5): only their names are reported.
            let value = if name.eq_ignore_ascii_case("cookie") {
                mask(&crate::cookies::mask_cookie_header(value), secrets)
            } else {
                mask_header(value, secrets)
            };
            (name.clone(), value)
        })
        .collect();
    if let Some(body) = &request.body
        && let Some(content_type) = &body.content_type
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".to_owned(), content_type.clone()));
    }
    SentRequest {
        method: request.method.clone(),
        url: mask(&request.url, secrets),
        headers,
        body_size: request.body.as_ref().map_or(0, |b| b.bytes.len() as u64),
    }
}

fn received(c: CompletedResponse) -> ReceivedResponse {
    ReceivedResponse {
        status: c.head.status,
        version: c.head.version,
        // Cookie values are session secrets (wave 5): `Set-Cookie` keeps name and attributes.
        headers: c
            .head
            .headers
            .into_iter()
            .map(|(name, value)| {
                let value = if name.eq_ignore_ascii_case("set-cookie") {
                    crate::cookies::mask_set_cookie(&value)
                } else {
                    value
                };
                (name, value)
            })
            .collect(),
        url: c.head.url,
        redirects: c.head.redirects,
        body: report_body(&c.body, c.truncated),
        size: c.size,
        truncated: c.truncated,
        timing: c.head.timing,
        total: c.total,
    }
}

/// UTF-8 bodies as text, anything else as base64. A truncated body that is valid UTF-8 except
/// for a character cut at the limit is reported as text without that partial character.
fn report_body(bytes: &[u8], truncated: bool) -> ReportBody {
    match std::str::from_utf8(bytes) {
        Ok(s) => ReportBody::Utf8(s.to_owned()),
        Err(e) if truncated && e.error_len().is_none() => {
            ReportBody::Utf8(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned())
        }
        Err(_) => ReportBody::Base64(base64_encode(bytes)),
    }
}

// ---------------------------------------------------------------------------------------------
// Secret masking

/// Values that must not appear in the report: every value of a variable that is secret in any
/// layer, declared secret anywhere in the collection, or reported secret by `prepare`.
/// Longest first, so a secret that contains another one is masked as a whole.
fn secret_values(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    prepared: &Prepared,
) -> Vec<String> {
    let mut names: Vec<&str> = declared_vars(context, input)
        .filter(|v| v.secret)
        .map(|v| v.name.as_str())
        .collect();
    names.extend(
        prepared
            .used
            .iter()
            .filter(|u| u.secret)
            .map(|u| u.name.as_str()),
    );
    let mut values: Vec<String> = Vec::new();
    for layer in &prepared.scope.layers {
        for (name, value, secret) in &layer.vars {
            if *secret || names.contains(&name.as_str()) {
                values.push(value.clone());
            }
        }
    }
    for (name, value) in input.globals.iter().chain(&input.runtime) {
        if names.contains(&name.as_str()) {
            values.push(value.clone());
        }
    }
    values.retain(|v| !v.is_empty());
    values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    values.dedup();
    values
}

/// Variables declared in the files that apply to this request (collection, the selected
/// environment, folders, request), lowest precedence first.
fn declared_vars<'a>(
    context: &'a RequestContext<'_>,
    input: &PrepareInput<'_>,
) -> impl Iterator<Item = &'a Variable> {
    let environment = input
        .environment
        .and_then(|name| context.collection.environment(name));
    context
        .collection
        .defaults
        .vars
        .iter()
        .chain(environment.into_iter().flat_map(|e| e.variables.iter()))
        .chain(context.folders.iter().flat_map(|f| f.defaults.vars.iter()))
        .chain(context.request.spec.vars.iter())
}

/// Replaces every secret, raw, percent-encoded or Debug-escaped (as in `{:?}` messages), with
/// [`MASK`].
fn mask(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        for form in [
            secret.clone(),
            secret.escape_debug().to_string(),
            percent_encode(secret, false),
            percent_encode(secret, true),
        ] {
            if !form.is_empty() && out.contains(&form) {
                out = out.replace(&form, MASK);
            }
        }
    }
    out
}

/// Like [`mask`]; additionally hides HTTP Basic credentials that contain a secret, since
/// base64 hides the secret from plain substring matching.
fn mask_header(value: &str, secrets: &[String]) -> String {
    if let Some((scheme, credentials)) = value.split_once(' ')
        && scheme.eq_ignore_ascii_case("basic")
        && let Some(decoded) = base64_decode(credentials.trim())
    {
        let decoded = String::from_utf8_lossy(&decoded);
        if secrets.iter().any(|s| decoded.contains(s.as_str())) {
            return format!("{scheme} {MASK}");
        }
    }
    mask(value, secrets)
}

/// RFC 3986 encoding of everything but unreserved characters; `plus` encodes space as `+`
/// (form encoding).
fn percent_encode(s: &str, plus: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' if plus => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Unresolved placeholders when preparation failed

/// `prepare` reports unresolved names only on success. When it fails (typically because an
/// unresolved placeholder left the URL invalid), the report still names them: this scans the
/// request's templates and the variables declared for it, following variable values
/// recursively. Dynamic or script-provided variables are not known here.
fn unresolved_fallback(context: &RequestContext<'_>, input: &PrepareInput<'_>) -> Vec<String> {
    let mut defined: Vec<(&str, &str)> = input
        .globals
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    defined.extend(
        declared_vars(context, input)
            .filter(|v| v.enabled)
            .filter_map(|v| Some((v.name.as_str(), v.value.as_deref()?))),
    );
    defined.extend(input.runtime.iter().map(|(n, v)| (n.as_str(), v.as_str())));
    // Highest precedence last: look up from the end.
    let lookup = |name: &str| {
        defined
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
    };

    let spec = &context.request.spec;
    let mut templates: Vec<&str> = vec![&spec.url];
    for p in spec.params.iter().filter(|p| p.enabled) {
        templates.extend([p.name.as_str(), p.value.as_str()]);
    }
    let header_sets = std::iter::once(&context.collection.defaults.headers)
        .chain(context.folders.iter().map(|f| &f.defaults.headers))
        .chain(std::iter::once(&spec.headers));
    for kv in header_sets.flatten().filter(|kv| kv.enabled) {
        templates.extend([kv.name.as_str(), kv.value.as_str()]);
    }
    let auth = std::iter::once(&spec.auth)
        .chain(context.folders.iter().rev().map(|f| &f.defaults.auth))
        .chain(std::iter::once(&context.collection.defaults.auth))
        .find(|a| !matches!(a, Auth::Inherit));
    match auth {
        Some(Auth::Basic { username, password }) => templates.extend([&**username, password]),
        Some(Auth::Bearer { token }) => templates.push(token),
        Some(Auth::ApiKey { key, value, .. }) => templates.extend([&**key, value]),
        _ => {}
    }
    match &spec.body {
        Body::Raw { data, .. } => templates.push(data),
        Body::FormUrlEncoded { fields } => {
            for kv in fields.iter().filter(|kv| kv.enabled) {
                templates.extend([kv.name.as_str(), kv.value.as_str()]);
            }
        }
        Body::Multipart { parts } => {
            for part in parts.iter().filter(|p| p.enabled) {
                templates.push(&part.name);
                templates.extend(part.values.iter().map(String::as_str));
            }
        }
        Body::File { path, .. } => templates.push(path),
        Body::GraphQl { query, variables } => templates.extend([&**query, variables]),
        Body::None => {}
    }

    let mut unresolved = Vec::new();
    let mut visited = Vec::new();
    let mut stack: Vec<&str> = templates.into_iter().rev().collect();
    while let Some(template) = stack.pop() {
        let mut nested = Vec::new();
        for name in placeholders(template) {
            match lookup(name) {
                Some(value) => {
                    if !visited.contains(&name) {
                        visited.push(name);
                        nested.push(value);
                    }
                }
                None => {
                    if !unresolved.iter().any(|u: &String| u == name) {
                        unresolved.push(name.to_owned());
                    }
                }
            }
        }
        stack.extend(nested.into_iter().rev());
    }
    unresolved
}

/// Names inside `{{...}}`, trimmed, in order.
fn placeholders(template: &str) -> impl Iterator<Item = &str> {
    let mut rest = template;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find("{{")?;
            let after = &rest[start + 2..];
            let end = after.find("}}")?;
            let name = after[..end].trim();
            rest = &after[end + 2..];
            if !name.is_empty() && !name.contains('{') {
                return Some(name);
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Base64 (standard alphabet, padded). Small enough to not need a dependency.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = match chunk {
            [a, b, c] => u32::from_be_bytes([0, *a, *b, *c]),
            [a, b] => u32::from_be_bytes([0, *a, *b, 0]),
            [a] => u32::from_be_bytes([0, *a, 0, 0]),
            _ => unreachable!(),
        };
        let sextet = |i: u32| B64[((n >> (18 - 6 * i)) & 63) as usize] as char;
        out.push(sextet(0));
        out.push(sextet(1));
        out.push(if chunk.len() > 1 { sextet(2) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(3) } else { '=' });
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use bytes::Bytes;

    use super::*;
    use crate::model::{
        Environment, Folder, Item, KeyValue, RequestDefaults, RequestEntry, RequestSpec,
    };
    use crate::transport::{BodyStream, HttpBody, Response, ResponseHead};
    use crate::vars::{Scope, VarLayer, VarSource};

    /// The mock futures are always ready, so a busy poll with a no-op waker is enough.
    fn block_on<F: Future>(fut: F) -> F::Output {
        let mut fut = std::pin::pin!(fut);
        let mut cx = Context::from_waker(std::task::Waker::noop());
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    struct Chunks(VecDeque<Bytes>);

    impl futures_core::Stream for Chunks {
        type Item = Result<Bytes, TransportError>;
        fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.pop_front().map(Ok))
        }
    }

    /// Records the request it was given and answers with a fixed result.
    struct MockTransport {
        answer: Result<(u16, Vec<&'static [u8]>), TransportError>,
        seen: std::cell::RefCell<Option<HttpRequest>>,
    }

    impl MockTransport {
        fn new(answer: Result<(u16, Vec<&'static [u8]>), TransportError>) -> Self {
            Self {
                answer,
                seen: Default::default(),
            }
        }
    }

    impl Transport for MockTransport {
        async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
            let url = request.url.clone();
            *self.seen.borrow_mut() = Some(request);
            let (status, chunks) = self.answer.clone()?;
            let body: BodyStream =
                Box::pin(Chunks(chunks.into_iter().map(Bytes::from_static).collect()));
            Ok(Response {
                head: ResponseHead {
                    status,
                    version: HttpVersion::Http11,
                    headers: vec![("content-type".into(), "text/plain".into())],
                    url,
                    redirects: vec![],
                    timing: Timing::default(),
                },
                body,
            })
        }
    }

    struct NoFiles;

    impl FileSource for NoFiles {
        fn read(&self, path: &str) -> Result<Vec<u8>, String> {
            Err(format!("no file {path}"))
        }
    }

    fn secret(name: &str) -> Variable {
        Variable {
            name: name.into(),
            value: None,
            enabled: true,
            secret: true,
        }
    }

    fn collection() -> Collection {
        let request = RequestEntry {
            path: "api/get.yml".into(),
            spec: RequestSpec {
                name: "get".into(),
                method: "GET".into(),
                url: "{{baseUrl}}/items?key={{apiKey}}".into(),
                headers: vec![
                    KeyValue::new("X-Key", "{{apiKey}}"),
                    KeyValue::new("X-Missing", "{{nope}}"),
                ],
                ..Default::default()
            },
        };
        Collection {
            config: Default::default(),
            name: "test".into(),
            defaults: RequestDefaults {
                vars: vec![Variable::new("baseUrl", "{{scheme}}://{{host}}")],
                ..Default::default()
            },
            items: vec![Item::Folder(Folder {
                name: "api".into(),
                path: "api".into(),
                defaults: RequestDefaults {
                    auth: Auth::Bearer {
                        token: "{{token}}".into(),
                    },
                    ..Default::default()
                },
                items: vec![Item::Request(request)],
                ..Default::default()
            })],
            environments: vec![Environment {
                client_certificates: Vec::new(),
                external_secrets: None,
                name: "Local".into(),
                variables: vec![Variable::new("scheme", "http"), secret("apiKey")],
            }],
        }
    }

    fn input() -> PrepareInput<'static> {
        PrepareInput {
            environment: Some("Local"),
            globals: vec![],
            runtime: vec![("apiKey".into(), "s3cr3t value".into())],
        }
    }

    /// What strand C's `prepare` would produce for `collection()` with `input()`.
    fn prepared() -> Prepared {
        Prepared {
            request: HttpRequest {
                method: "POST".into(),
                url: "http://localhost/items?key=s3cr3t%20value".into(),
                headers: vec![
                    ("X-Key".into(), "s3cr3t value".into()),
                    ("X-Missing".into(), "{{nope}}".into()),
                    ("Authorization".into(), "Bearer tok".into()),
                ],
                body: Some(HttpBody {
                    content_type: Some("application/json".into()),
                    bytes: Bytes::from_static(b"{}"),
                }),
                options: Default::default(),
            },
            auth: Auth::Bearer {
                token: "{{token}}".into(),
            },
            scope: Scope {
                layers: vec![VarLayer {
                    source: VarSource::Runtime,
                    vars: vec![("apiKey".into(), "s3cr3t value".into(), false)],
                }],
            },
            used: vec![VarUse {
                name: "apiKey".into(),
                source: VarSource::Runtime,
                secret: false,
            }],
            unresolved: vec!["nope".into()],
        }
    }

    fn base_report() -> SendReport {
        SendReport {
            tests: Vec::new(),
            assertions: Vec::new(),
            logs: Vec::new(),
            var_changes: Vec::new(),
            flow: None,
            notes: Vec::new(),
            version: REPORT_VERSION,
            request_path: "api/get.yml".into(),
            environment: Some("Local".into()),
            request: None,
            response: None,
            variables: vec![],
            unresolved: vec![],
            error: None,
        }
    }

    #[test]
    fn unknown_request_is_not_found() {
        let c = collection();
        let t = MockTransport::new(Err(TransportError::Timeout));
        let r = block_on(send(
            &c,
            "api/missing.yml",
            &input(),
            &NoFiles,
            &t,
            &SendOptions::default(),
        ));
        assert_eq!(r.version, REPORT_VERSION);
        assert_eq!(r.environment.as_deref(), Some("Local"));
        assert_eq!(r.error, Some(SendError::NotFound("api/missing.yml".into())));
        assert!(r.request.is_none() && r.response.is_none());
        assert!(t.seen.borrow().is_none());
    }

    #[test]
    fn prepare_error_still_reports_unresolved() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let t = MockTransport::new(Err(TransportError::Timeout));
        let err = PrepareError::InvalidUrl("http://{{host}}/items".into());
        let r = block_on(finish(
            base_report(),
            &ctx,
            &input(),
            &NoFiles,
            Err(err.clone()),
            &t,
            &SendOptions::default(),
            &Services::none(),
        ));
        assert_eq!(r.error, Some(SendError::Prepare(err.to_string())));
        // baseUrl -> {{scheme}}://{{host}}: host is missing; header {{nope}}; auth {{token}}.
        assert_eq!(r.unresolved, ["host", "nope", "token"]);
        assert!(r.request.is_none());
        assert!(t.seen.borrow().is_none(), "nothing may be sent");
    }

    #[test]
    fn transport_error_keeps_the_sent_request_and_masks_secrets() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let t = MockTransport::new(Err(TransportError::Connect("refused".into())));
        let r = block_on(finish(
            base_report(),
            &ctx,
            &input(),
            &NoFiles,
            Ok(prepared()),
            &t,
            &SendOptions::default(),
            &Services::none(),
        ));
        assert_eq!(
            r.error,
            Some(SendError::Transport(TransportError::Connect(
                "refused".into()
            )))
        );
        assert!(r.response.is_none());
        assert_eq!(r.unresolved, ["nope"]);
        assert_eq!(r.variables.len(), 1);
        let sent = r.request.unwrap();
        assert_eq!(sent.method, "POST");
        // apiKey is declared secret in the environment, so its runtime value is masked.
        assert_eq!(sent.url, "http://localhost/items?key=***");
        assert_eq!(
            sent.headers,
            [
                ("X-Key".to_owned(), "***".to_owned()),
                ("X-Missing".to_owned(), "{{nope}}".to_owned()),
                ("Authorization".to_owned(), "Bearer tok".to_owned()),
                ("Content-Type".to_owned(), "application/json".to_owned()),
            ]
        );
        assert_eq!(sent.body_size, 2);
        // The transport gets the real values.
        let seen = t.seen.borrow().clone().unwrap();
        assert_eq!(seen.headers[0].1, "s3cr3t value");
    }

    #[test]
    fn secret_values_come_from_every_source() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let mut p = prepared();
        p.scope.layers.push(VarLayer {
            source: VarSource::Request,
            vars: vec![("pw".into(), "hunter2".into(), true)],
        });
        p.used.push(VarUse {
            name: "other".into(),
            source: VarSource::Global,
            secret: true,
        });
        p.scope.layers[0]
            .vars
            .push(("other".into(), "zzz".into(), false));
        let s = secret_values(&ctx, &input(), &p);
        assert_eq!(s, ["s3cr3t value", "hunter2", "zzz"]);
    }

    #[test]
    fn transport_errors_never_carry_secret_values() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let secret = "s3cr3t value";
        for error in [
            TransportError::InvalidUrl(format!("ftp://x/{secret}: only http")),
            TransportError::Dns(format!("{secret}.example: not found")),
            TransportError::Connect(format!("http://localhost/items?key={}", "s3cr3t%20value")),
            TransportError::Tls(secret.into()),
            TransportError::Blocked(secret.into()),
            TransportError::Other(format!(
                "invalid value for header X-Key: {:?}",
                "s3cr3t value\nx"
            )),
        ] {
            let t = MockTransport::new(Err(error));
            let r = block_on(finish(
                base_report(),
                &ctx,
                &input(),
                &NoFiles,
                Ok(prepared()),
                &t,
                &SendOptions::default(),
                &Services::none(),
            ));
            let json = serde_json::to_string(&r.error).unwrap();
            assert!(!json.contains("s3cr3t"), "{json}");
            assert!(json.contains(MASK), "{json}");
        }
    }

    #[test]
    fn masking_debug_escaped_values() {
        let secrets = vec!["a\nb\"q".to_owned()];
        let quoted = format!("header X: {:?}", "a\nb\"q");
        assert_eq!(mask(&quoted, &secrets), "header X: \"***\"");
    }

    #[test]
    fn masking() {
        let secrets = vec!["p@ss word".to_owned(), "tok".to_owned()];
        assert_eq!(mask("a=p%40ss%20word&b=tok", &secrets), "a=***&b=***");
        assert_eq!(mask("a=p%40ss+word", &secrets), "a=***");
        assert_eq!(mask("nothing here", &secrets), "nothing here");
        let basic = format!("Basic {}", base64_encode(b"alice:p@ss word"));
        assert_eq!(mask_header(&basic, &secrets), "Basic ***");
        let plain = format!("Basic {}", base64_encode(b"alice:public"));
        assert_eq!(mask_header(&plain, &secrets), plain);
        assert_eq!(mask_header("Bearer tok", &secrets), "Bearer ***");
        assert_eq!(mask("x", &[]), "x");
    }

    #[test]
    fn body_encoding() {
        assert_eq!(report_body(b"hi", false), ReportBody::Utf8("hi".into()));
        assert_eq!(
            report_body(&[0xff, 0x00], false),
            ReportBody::Base64("/wA=".into())
        );
        // "é" is c3 a9; cut after c3.
        assert_eq!(
            report_body(&[b'a', 0xc3], true),
            ReportBody::Utf8("a".into())
        );
        assert_eq!(
            report_body(&[b'a', 0xc3], false),
            ReportBody::Base64("YcM=".into())
        );
    }

    #[test]
    fn report_json_shape() {
        let body = serde_json::to_value(ReportBody::Utf8("x".into())).unwrap();
        assert_eq!(body, serde_json::json!({"encoding": "utf8", "data": "x"}));
        let err = serde_json::to_value(SendError::NotFound("a.yml".into())).unwrap();
        assert_eq!(
            err,
            serde_json::json!({"kind": "not-found", "message": "a.yml"})
        );
        let err = serde_json::to_value(SendError::Transport(TransportError::Timeout)).unwrap();
        assert_eq!(
            err,
            serde_json::json!({"kind": "transport", "message": {"kind": "timeout"}})
        );
    }

    #[test]
    fn received_maps_every_field() {
        let c = CompletedResponse {
            head: ResponseHead {
                status: 404,
                version: HttpVersion::Http2,
                headers: vec![("a".into(), "b".into())],
                url: "http://x/final".into(),
                redirects: vec![Redirect {
                    status: 302,
                    location: "/final".into(),
                }],
                timing: Timing {
                    ttfb: Duration::from_millis(3),
                    ..Default::default()
                },
            },
            body: Bytes::from_static(b"abc"),
            truncated: true,
            size: 10,
            total: Duration::from_millis(7),
        };
        let r = received(c);
        assert_eq!(r.status, 404);
        assert_eq!(r.version, HttpVersion::Http2);
        assert_eq!(r.url, "http://x/final");
        assert_eq!(r.redirects.len(), 1);
        assert_eq!(r.body, ReportBody::Utf8("abc".into()));
        assert_eq!((r.size, r.truncated), (10, true));
        assert_eq!(r.timing.ttfb, Duration::from_millis(3));
        assert_eq!(r.total, Duration::from_millis(7));
    }

    #[test]
    fn base64_round_trip() {
        for (raw, enc) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(raw), enc);
            assert_eq!(base64_decode(enc).unwrap(), raw);
        }
        assert!(base64_decode("not base64!").is_none());
    }

    #[test]
    fn placeholder_scan() {
        let names: Vec<_> = placeholders("{{a}}/{{ b }}/{{}}{{c").collect();
        assert_eq!(names, ["a", "b"]);
    }

    /// The success path runs through `transport::collect` (strand A).
    #[test]
    fn response_is_collected_and_reported() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let t = MockTransport::new(Ok((200, vec![b"hel", b"lo"])));
        let r = block_on(finish(
            base_report(),
            &ctx,
            &input(),
            &NoFiles,
            Ok(prepared()),
            &t,
            &SendOptions {
                iteration: None,
                body_limit: 4,
            },
            &Services::none(),
        ));
        assert_eq!(r.error, None);
        let resp = r.response.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, ReportBody::Utf8("hell".into()));
        assert_eq!((resp.size, resp.truncated), (5, true));
        assert!(resp.total >= resp.timing.ttfb);
    }

    /// End to end through `prepare` (strand C) and `collect` (strand A).
    #[test]
    fn send_prepares_sends_and_masks() {
        let c = collection();
        let t = MockTransport::new(Ok((201, vec![b"ok"])));
        let mut input = input();
        input.runtime.push(("host".into(), "localhost".into()));
        input.runtime.push(("token".into(), "tok".into()));
        let r = block_on(send(
            &c,
            "api/get.yml",
            &input,
            &NoFiles,
            &t,
            &SendOptions::default(),
        ));
        assert_eq!(r.error, None, "{r:?}");
        assert_eq!(r.unresolved, ["nope"]);
        let sent = r.request.unwrap();
        assert!(
            sent.url.starts_with("http://localhost/items?key="),
            "{}",
            sent.url
        );
        assert!(!sent.url.contains("s3cr3t"), "{}", sent.url);
        assert!(
            sent.headers
                .iter()
                .any(|(n, v)| n.eq_ignore_ascii_case("x-key") && v == MASK),
            "{:?}",
            sent.headers
        );
        let seen = t.seen.borrow().clone().unwrap();
        assert!(
            seen.headers
                .iter()
                .any(|(n, v)| n.eq_ignore_ascii_case("authorization") && v == "Bearer tok")
        );
        assert_eq!(r.response.unwrap().status, 201);
    }
}
