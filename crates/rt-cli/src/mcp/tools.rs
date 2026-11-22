// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The tools of `rt mcp` (contract: `tests/gate_w8.rs`).
//!
//! | Tool | Arguments | Result (text JSON) |
//! | --- | --- | --- |
//! | `list_requests` | – | `[{"path", "name", "method", "effect"}]` |
//! | `get_request` | `path` | the request (`RequestSpec`), credentials masked |
//! | `list_environments` | – | `[{"name", "access", "production", "variables": [{"name", "secret", "value"?}]}]` |
//! | `send_request` | `path`, `environment`?, `vars`? | the masked `SendReport` |
//! | `run_folder` | `folder`?, `environment`?, `vars`? | `{"summary", "steps": [{…, "report"}]}` |
//!
//! Every call is read from the files anew (the agent may have edited them), with the
//! policy of `policy`. Each `tools/call` writes exactly one audit entry. Refusals, denials and
//! timeouts are results with `isError: true` and say that nothing was sent.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use rt_app::agents::{self, AuditOutcome, Effect, PendingApproval, Verdict};
use rt_core::model::{AgentAccess, AgentPolicy, Collection, Item, Variable};
use rt_core::prepare::PrepareInput;
use rt_core::runner::{RunEvent, RunPlan, RunStep};
use rt_core::send::{SendError, SendOptions, SendReport};
use rt_core::transport::{HttpRequest, Response, Transport, TransportError};
use rt_format::CollectionDoc;
use serde_json::{Value, json};

use super::approval::{self, Outcome};
use super::mask::spec_for_agent;
use super::policy;
use super::server::{INVALID_PARAMS, RpcError, Server};
use crate::{DirFiles, SendArgs};

/// `tools/list`.
pub fn list() -> Value {
    let env_arg = json!({
        "type": "string",
        "description": "Environment name (see list_environments); default: the server's --env-default"
    });
    let vars_arg = json!({
        "type": "object",
        "additionalProperties": {"type": "string"},
        "description": "Variables for this call (request variables or names nothing else defines)"
    });
    json!({"tools": [
        {
            "name": "list_requests",
            "title": "List requests",
            "description": "All requests of the collection: path, name, method and whether sending it reads or writes.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"title": "List requests", "readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "get_request",
            "title": "Show a request",
            "description": "One request as in its file (URL, headers, body, auth, scripts). Secrets appear only as {{variable}} names.",
            "inputSchema": {
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Request path from list_requests"}},
                "required": ["path"]
            },
            "annotations": {"title": "Show a request", "readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "list_environments",
            "title": "List environments",
            "description": "Environments the agent may use, with access (read or write), the production flag and variable names; secret values are never shown.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"title": "List environments", "readOnlyHint": true, "openWorldHint": false}
        },
        {
            "name": "send_request",
            "title": "Send a request",
            "description": "Sends one request and returns the report (status, headers, body, tests); secrets masked. Writing requests need write access; against production they wait for the user's approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Request path from list_requests"},
                    "environment": env_arg,
                    "vars": vars_arg
                },
                "required": ["path"]
            },
            "annotations": {"title": "Send a request", "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": true}
        },
        {
            "name": "run_folder",
            "title": "Run a folder",
            "description": "Runs the requests of a folder (or the whole collection) in order, like `rt run`, and returns every report. Each request is checked like send_request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder": {"type": "string", "description": "Folder path, e.g. orders; empty or missing: the whole collection"},
                    "environment": env_arg,
                    "vars": vars_arg
                }
            },
            "annotations": {"title": "Run a folder", "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": true}
        }
    ]})
}

/// What a tool produced.
struct ToolResult {
    text: String,
    is_error: bool,
}

impl ToolResult {
    fn ok(value: &Value) -> Self {
        Self {
            text: value.to_string(),
            is_error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

fn result_value(r: ToolResult) -> Value {
    json!({"content": [{"type": "text", "text": r.text}], "isError": r.is_error})
}

/// `tools/call`.
pub async fn call(server: &Server<'_>, id: &Value, params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let empty = json!({});
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(a) if a.is_object() => a,
        Some(_) => {
            return Err(fail_args(server, &name, "arguments must be an object"));
        }
    };
    let mut entry = server.audit_entry(&name, AuditOutcome::Read);
    let result = match name.as_str() {
        "list_requests" => Ok(list_requests(server)),
        "get_request" => get_request(server, args, &mut entry),
        "list_environments" => Ok(list_environments(server)),
        "send_request" => send_request(server, id, args, &mut entry).await,
        "run_folder" => run_folder(server, id, args, &mut entry).await,
        _ => {
            entry.outcome = AuditOutcome::Failed;
            entry.detail = "unknown tool".into();
            server.audit(entry);
            return Err(RpcError::new(
                INVALID_PARAMS,
                format!("unknown tool: {name}"),
            ));
        }
    };
    match result {
        Ok(r) => {
            if r.is_error && entry.outcome == AuditOutcome::Read {
                entry.outcome = AuditOutcome::Failed;
                entry.detail = r.text.chars().take(300).collect();
            }
            server.audit(entry);
            Ok(result_value(r))
        }
        Err(message) => {
            entry.outcome = AuditOutcome::Failed;
            entry.detail = message.clone();
            server.audit(entry);
            Err(RpcError::new(INVALID_PARAMS, message))
        }
    }
}

fn fail_args(server: &Server<'_>, tool: &str, message: &str) -> RpcError {
    let mut entry = server.audit_entry(tool, AuditOutcome::Failed);
    entry.detail = message.to_owned();
    server.audit(entry);
    RpcError::new(INVALID_PARAMS, message)
}

/// A string argument; `Err` if present but not a string.
fn str_arg<'v>(args: &'v Value, name: &str) -> Result<Option<&'v str>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("`{name}` must be a string")),
    }
}

/// `vars` as pairs; non-string values as their JSON text.
fn vars_arg(args: &Value) -> Result<Vec<(String, String)>, String> {
    match args.get("vars") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(map)) => Ok(map
            .iter()
            .map(|(k, v)| {
                let value = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                (k.clone(), value)
            })
            .collect()),
        Some(_) => Err("`vars` must be an object of names and values".into()),
    }
}

fn load(server: &Server<'_>) -> Result<CollectionDoc, ToolResult> {
    crate::load_collection(&server.root).map_err(ToolResult::error)
}

/// The policy in force for this call.
fn policy_of(server: &Server<'_>, doc: &CollectionDoc) -> AgentPolicy {
    policy::tighten(&server.pinned, &doc.model().config.agents)
}

fn access_word(a: AgentAccess) -> &'static str {
    match a {
        AgentAccess::None => "none",
        AgentAccess::Read => "read",
        AgentAccess::Write => "write",
    }
}

fn effect_word(e: Effect) -> &'static str {
    match e {
        Effect::Read => "read",
        Effect::Write => "write",
    }
}

fn list_requests(server: &Server<'_>) -> ToolResult {
    let doc = match load(server) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let model = doc.model();
    let list: Vec<Value> = rt_core::runner::plan(model, None)
        .into_iter()
        .filter_map(|path| {
            let context = model.request(&path)?;
            let spec = &context.request.spec;
            Some(json!({
                "path": path,
                "name": spec.name,
                "method": spec.method,
                "effect": effect_word(agents::effect(spec)),
            }))
        })
        .collect();
    ToolResult::ok(&Value::Array(list))
}

fn get_request(
    server: &Server<'_>,
    args: &Value,
    entry: &mut rt_app::agents::AuditEntry,
) -> Result<ToolResult, String> {
    let path = str_arg(args, "path")?.ok_or("`path` is required")?;
    entry.request_path = Some(path.to_owned());
    let doc = match load(server) {
        Ok(d) => d,
        Err(e) => return Ok(e),
    };
    let Some(context) = doc.model().request(path) else {
        return Ok(ToolResult::error(format!(
            "no request {path} in the collection (see list_requests)"
        )));
    };
    let spec = &context.request.spec;
    entry.method = Some(spec.method.clone());
    let mut value = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    spec_for_agent(&mut value);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("path".into(), json!(path));
        obj.insert("effect".into(), json!(effect_word(agents::effect(spec))));
    }
    Ok(ToolResult::ok(&value))
}

fn list_environments(server: &Server<'_>) -> ToolResult {
    let doc = match load(server) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let policy = policy_of(server, &doc);
    let list: Vec<Value> = doc
        .model()
        .environments
        .iter()
        .filter(|e| agents::visible(&policy, &e.name))
        .map(|e| {
            let (access, production) = policy.for_environment(Some(&e.name));
            let variables: Vec<Value> = e
                .variables
                .iter()
                .filter(|v| v.enabled)
                .map(|v| {
                    if v.secret {
                        json!({"name": v.name, "secret": true})
                    } else {
                        json!({"name": v.name, "secret": false, "value": v.value})
                    }
                })
                .collect();
            json!({
                "name": e.name,
                "access": access_word(access),
                "production": production,
                "variables": variables,
            })
        })
        .collect();
    ToolResult::ok(&Value::Array(list))
}

/// What a send needs, with secrets filled in.
struct Prepared {
    collection: Collection,
    environment: Option<String>,
    /// `--var` and the agent's variables.
    runtime: Vec<(String, String)>,
    args: SendArgs,
    trusted: bool,
}

/// Whether the user trusted this collection's scripts in the desktop app.
fn trusted(server: &Server<'_>) -> bool {
    let file = server.data_dir.join("trusted-scripts.json");
    std::fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| {
            v.get("trusted")
                .and_then(Value::as_array)
                .map(|list| list.iter().any(|id| id == server.collection_id.as_str()))
        })
        .unwrap_or(false)
}

/// The environment for a call and its check against the policy and the collection.
fn environment_for(
    server: &Server<'_>,
    doc: &CollectionDoc,
    policy: &AgentPolicy,
    args: &Value,
) -> Result<Option<String>, String> {
    let env = str_arg(args, "environment")?
        .map(str::to_owned)
        .or_else(|| server.opts.env_default.clone())
        .filter(|e| !e.is_empty());
    if let Some(name) = &env {
        let exists = doc.model().environment(name).is_some();
        if !exists || !agents::visible(policy, name) {
            let names: Vec<&str> = doc
                .model()
                .environments
                .iter()
                .map(|e| e.name.as_str())
                .filter(|n| agents::visible(policy, n))
                .collect();
            return Err(format!(
                "no environment {name} for agents (available: {})",
                if names.is_empty() {
                    "none".to_owned()
                } else {
                    names.join(", ")
                }
            ));
        }
    }
    Ok(env)
}

/// Fills in secret values (keychain, secret managers) and marks every `--var` as secret.
async fn prepare(
    server: &Server<'_>,
    doc: &CollectionDoc,
    environment: Option<String>,
    agent_vars: Vec<(String, String)>,
) -> Result<Prepared, String> {
    let mut collection = doc.model().clone();
    let given = &server.opts.vars;
    let mut skip: Vec<(String, String)> = given.clone();
    // Keychain values the desktop app stored for this collection (keychain wins over a
    // secret manager, as in the app).
    if let Some(env) = environment
        .as_deref()
        .and_then(|n| collection.environments.iter_mut().find(|e| e.name == n))
    {
        let store = rt_app::secrets::keychain::KeychainStore::default();
        for var in env
            .variables
            .iter_mut()
            .filter(|v| v.secret && v.value.is_none())
        {
            if given.iter().any(|(n, _)| *n == var.name) {
                continue;
            }
            let key = rt_app::secrets::SecretKey {
                collection: server.collection_id.clone(),
                environment: env.name.clone(),
                variable: var.name.clone(),
            };
            use rt_app::secrets::SecretStore as _;
            if let Ok(Some(value)) = store.get(&key) {
                skip.push((var.name.clone(), String::new()));
                var.value = Some(value);
            }
        }
    }
    let mut collection =
        crate::secrets::resolve(&collection, environment.as_deref(), &skip).await?;
    {
        let mut masker = server.masker.borrow_mut();
        for env in &collection.environments {
            for v in env.variables.iter().filter(|v| v.secret) {
                if let Some(value) = &v.value {
                    masker.add(value);
                }
            }
        }
    }
    // `--var` values are secret: declared so (without a value) the core masks them too.
    for (name, _) in given {
        collection.defaults.vars.push(Variable {
            name: name.clone(),
            value: None,
            enabled: true,
            secret: true,
        });
    }
    let mut runtime = given.clone();
    runtime.extend(agent_vars);
    let trusted = !server.opts.no_scripts && trusted(server);
    let args = SendArgs {
        file: PathBuf::new(),
        environment: environment.clone(),
        vars: runtime.clone(),
        body_limit: server.opts.body_limit,
        pretty: false,
        plugins: Vec::new(),
        no_plugins: false,
        cookie_jar: None,
        // Nobody sees a terminal prompt; OAuth flows that need the user fail.
        prompt: false,
        no_scripts: !trusted,
        // Sessions (WebSocket, SSE, gRPC) are not offered to agents yet.
        session: Default::default(),
    };
    Ok(Prepared {
        collection,
        environment,
        runtime,
        args,
        trusted,
    })
}

/// The request's URL for the approval banner, masked; the URL template if it cannot be
/// prepared.
fn preview(
    server: &Server<'_>,
    p: &Prepared,
    request_path: &str,
    files: &DirFiles,
) -> (String, String) {
    let Some(context) = p.collection.request(request_path) else {
        return (String::new(), String::new());
    };
    let input = PrepareInput {
        environment: p.environment.as_deref(),
        globals: Vec::new(),
        runtime: p.runtime.clone(),
    };
    let spec = &context.request.spec;
    let url = match rt_core::prepare::prepare(&context, &input, files) {
        Ok(prepared) => {
            let secrets: Vec<String> = prepared
                .scope
                .layers
                .iter()
                .flat_map(|l| &l.vars)
                .filter(|(_, _, secret)| *secret)
                .map(|(_, v, _)| v.clone())
                .collect();
            let mut url = prepared.request.url;
            for s in secrets.iter().filter(|s| !s.is_empty()) {
                url = url.replace(s.as_str(), rt_core::send::MASK);
            }
            url
        }
        Err(_) => spec.url.clone(),
    };
    (spec.method.clone(), server.masker.borrow().text(&url))
}

fn error_text(e: &SendError) -> String {
    match e {
        SendError::NotFound(p) => format!("request not found: {p}"),
        SendError::Prepare(m) => format!("cannot prepare the request: {m}"),
        SendError::Auth(m) => format!("authentication failed: {m}"),
        SendError::Plugin(m) => format!("stopped by a plugin: {m}"),
        SendError::Script(m) => format!("stopped by a script: {m}"),
        SendError::Transport(t) => t.to_string(),
    }
}

fn pending(
    server: &Server<'_>,
    doc: &CollectionDoc,
    request_path: &str,
    environment: Option<&str>,
    method: String,
    url: String,
) -> PendingApproval {
    PendingApproval {
        id: agents::new_id(),
        at: agents::now_ms(),
        agent: server.agent(),
        collection: server.root.display().to_string(),
        collection_name: if doc.model().name.is_empty() {
            server
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            doc.model().name.clone()
        },
        request_path: request_path.to_owned(),
        environment: environment.map(str::to_owned),
        method,
        url,
        reason: agents::approval_reason(environment),
    }
}

/// Text for an approval that did not end in "approved"; `None` for approved.
fn not_approved(outcome: &Outcome, timeout: u64) -> Option<(AuditOutcome, String)> {
    match outcome {
        Outcome::Approved(_) => None,
        Outcome::Denied(by) => Some((
            AuditOutcome::Denied,
            format!("the user denied this request (via {by}); nothing was sent"),
        )),
        Outcome::TimedOut => Some((
            AuditOutcome::TimedOut,
            format!(
                "no approval within {timeout} s: writing requests against production need the \
                 user's approval (app banner or `rt approve`); nothing was sent"
            ),
        )),
        Outcome::Cancelled => Some((
            AuditOutcome::Failed,
            "cancelled while waiting for approval; nothing was sent".into(),
        )),
        Outcome::Failed(e) => Some((AuditOutcome::Failed, format!("{e}; nothing was sent"))),
    }
}

async fn send_request(
    server: &Server<'_>,
    call_id: &Value,
    args: &Value,
    entry: &mut rt_app::agents::AuditEntry,
) -> Result<ToolResult, String> {
    let path = str_arg(args, "path")?
        .ok_or("`path` is required")?
        .to_owned();
    let agent_vars = vars_arg(args)?;
    entry.request_path = Some(path.clone());
    let doc = match load(server) {
        Ok(d) => d,
        Err(e) => return Ok(e),
    };
    let policy = policy_of(server, &doc);
    let environment = match environment_for(server, &doc, &policy, args) {
        Ok(e) => e,
        Err(reason) => {
            entry.outcome = AuditOutcome::Refused;
            entry.detail = reason.clone();
            return Ok(ToolResult::error(reason));
        }
    };
    entry.environment = environment.clone();
    let Some(context) = doc.model().request(&path) else {
        return Ok(ToolResult::error(format!(
            "no request {path} in the collection (see list_requests)"
        )));
    };
    let spec = &context.request.spec;
    entry.method = Some(spec.method.clone());
    let verdict = agents::verdict(&policy, environment.as_deref(), agents::effect(spec));
    if let Verdict::Refused { reason } = &verdict {
        entry.outcome = AuditOutcome::Refused;
        entry.detail = reason.clone();
        return Ok(ToolResult::error(format!("{reason}; nothing was sent")));
    }
    if let Err(reason) = policy::check_vars(
        doc.model(),
        Some(&path),
        environment.as_deref(),
        &server.opts.vars,
        &agent_vars,
    ) {
        entry.outcome = AuditOutcome::Refused;
        entry.detail = reason.clone();
        return Ok(ToolResult::error(format!("{reason}; nothing was sent")));
    }
    let files = DirFiles::new(&server.root).map_err(|e| e.to_string())?;
    let prepared = match prepare(server, &doc, environment.clone(), agent_vars).await {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(format!("cannot read secrets: {e}"))),
    };
    let (method, url) = preview(server, &prepared, &path, &files);
    entry.url = Some(url.clone());
    if verdict == Verdict::NeedsApproval {
        let approval = pending(server, &doc, &path, environment.as_deref(), method, url);
        let outcome = approval::wait(server, call_id, &approval).await;
        if let Some((audit, text)) = not_approved(&outcome, server.opts.approval_timeout) {
            entry.outcome = audit;
            entry.detail = text.clone();
            return Ok(ToolResult::error(text));
        }
        if let Outcome::Approved(by) = outcome {
            entry.approved_by = Some(by);
        }
    }
    let report = send_one(server, &prepared, &path, &files, &server.transport).await;
    record_send(entry, &report);
    let is_error = report.error.is_some();
    let mut value = serde_json::to_value(&report).map_err(|e| e.to_string())?;
    server.masker.borrow().value(&mut value);
    Ok(ToolResult {
        text: value.to_string(),
        is_error,
    })
}

fn record_send(entry: &mut rt_app::agents::AuditEntry, report: &SendReport) {
    if let Some(request) = &report.request {
        entry.method = Some(request.method.clone());
        entry.url = Some(request.url.clone());
    }
    match (&report.response, &report.error) {
        (Some(r), _) => {
            entry.outcome = AuditOutcome::Sent;
            entry.status = Some(r.status);
        }
        (None, Some(e)) => {
            entry.outcome = AuditOutcome::Failed;
            entry.detail = error_text(e);
        }
        (None, None) => entry.outcome = AuditOutcome::Failed,
    }
}

async fn send_one<T: Transport>(
    server: &Server<'_>,
    p: &Prepared,
    request_path: &str,
    files: &DirFiles,
    transport: &T,
) -> SendReport {
    let input = PrepareInput {
        environment: p.environment.as_deref(),
        globals: Vec::new(),
        runtime: p.runtime.clone(),
    };
    let options = SendOptions {
        iteration: None,
        body_limit: p.args.body_limit,
    };
    let cli = match crate::services::CliServices::new(&p.args, &server.root) {
        Ok(s) => s,
        Err(e) => return crate::services::failed(request_path, &p.args, SendError::Plugin(e)),
    };
    let mut report = rt_core::send::send_with(
        &p.collection,
        request_path,
        &input,
        files,
        transport,
        &options,
        &cli.services(),
    )
    .await;
    if !p.trusted && !server.opts.no_scripts {
        rt_app::scripts::note_untrusted(&mut report, &p.collection);
    }
    report
}

/// The transport of `run_folder`: checks each step before its first byte goes out.
struct Gate<'s, 'a> {
    server: &'s Server<'a>,
    doc: &'s CollectionDoc,
    prepared: &'s Prepared,
    files: &'s DirFiles,
    policy: &'s AgentPolicy,
    call_id: &'s Value,
    /// The step being sent and the decision for it, once made.
    current: RefCell<(String, Option<Result<(), String>>)>,
    /// Approvals of this run: request path and outcome.
    approvals: RefCell<Vec<(String, Outcome)>>,
    cancelled: Cell<bool>,
}

impl Gate<'_, '_> {
    async fn decide(&self, path: &str) -> Result<(), String> {
        let Some(context) = self.doc.model().request(path) else {
            return Ok(());
        };
        let env = self.prepared.environment.as_deref();
        match agents::verdict(self.policy, env, agents::effect(&context.request.spec)) {
            Verdict::Allowed => Ok(()),
            Verdict::Refused { reason } => Err(reason),
            Verdict::NeedsApproval => {
                let (method, url) = preview(self.server, self.prepared, path, self.files);
                let approval = pending(self.server, self.doc, path, env, method, url);
                let outcome = approval::wait(self.server, self.call_id, &approval).await;
                let result = match not_approved(&outcome, self.server.opts.approval_timeout) {
                    None => Ok(()),
                    Some((_, text)) => Err(text),
                };
                if outcome == Outcome::Cancelled {
                    self.cancelled.set(true);
                }
                self.approvals.borrow_mut().push((path.to_owned(), outcome));
                result
            }
        }
    }
}

impl Transport for Gate<'_, '_> {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        let (path, decided) = self.current.borrow().clone();
        let decision = match decided {
            Some(d) => d,
            None => {
                let d = self.decide(&path).await;
                self.current.borrow_mut().1 = Some(d.clone());
                d
            }
        };
        match decision {
            Ok(()) => self.server.transport.send(request).await,
            Err(reason) => Err(TransportError::Other(format!("not sent: {reason}"))),
        }
    }
}

async fn run_folder(
    server: &Server<'_>,
    call_id: &Value,
    args: &Value,
    entry: &mut rt_app::agents::AuditEntry,
) -> Result<ToolResult, String> {
    let folder = str_arg(args, "folder")?
        .unwrap_or_default()
        .trim_matches('/')
        .to_owned();
    let agent_vars = vars_arg(args)?;
    entry.request_path = (!folder.is_empty()).then(|| folder.clone());
    let doc = match load(server) {
        Ok(d) => d,
        Err(e) => return Ok(e),
    };
    let policy = policy_of(server, &doc);
    let environment = match environment_for(server, &doc, &policy, args) {
        Ok(e) => e,
        Err(reason) => {
            entry.outcome = AuditOutcome::Refused;
            entry.detail = reason.clone();
            return Ok(ToolResult::error(reason));
        }
    };
    entry.environment = environment.clone();
    if !folder.is_empty() && !has_folder(&doc.model().items, &folder) {
        return Ok(ToolResult::error(format!(
            "no folder {folder} in the collection"
        )));
    }
    let requests = rt_core::runner::plan(doc.model(), Some(&folder));
    if requests.is_empty() {
        return Ok(ToolResult::error("no requests to run"));
    }
    if let Err(reason) = policy::check_vars(
        doc.model(),
        None,
        environment.as_deref(),
        &server.opts.vars,
        &agent_vars,
    ) {
        entry.outcome = AuditOutcome::Refused;
        entry.detail = reason.clone();
        return Ok(ToolResult::error(format!("{reason}; nothing was sent")));
    }
    let files = DirFiles::new(&server.root).map_err(|e| e.to_string())?;
    let prepared = match prepare(server, &doc, environment.clone(), agent_vars).await {
        Ok(p) => p,
        Err(e) => return Ok(ToolResult::error(format!("cannot read secrets: {e}"))),
    };
    let cli = match crate::services::CliServices::new(&prepared.args, &server.root) {
        Ok(s) => s,
        Err(e) => return Ok(ToolResult::error(e)),
    };
    let gate = Gate {
        server,
        doc: &doc,
        prepared: &prepared,
        files: &files,
        policy: &policy,
        call_id,
        current: RefCell::new((String::new(), None)),
        approvals: RefCell::new(Vec::new()),
        cancelled: Cell::new(false),
    };
    let plan = RunPlan {
        requests,
        data: Vec::new(),
        iterations: 1,
        delay_ms: 0,
        bail: false,
    };
    let input = PrepareInput {
        environment: prepared.environment.as_deref(),
        globals: Vec::new(),
        runtime: prepared.runtime.clone(),
    };
    let options = SendOptions {
        iteration: None,
        body_limit: prepared.args.body_limit,
    };
    let steps: RefCell<Vec<RunStep>> = RefCell::new(Vec::new());
    let mut on_event = |event: RunEvent| match event {
        RunEvent::Request { request_path, .. } => {
            *gate.current.borrow_mut() = (request_path, None);
        }
        RunEvent::Step { step } => steps.borrow_mut().push(*step),
        RunEvent::Started { .. } | RunEvent::Finished { .. } => {}
    };
    let cancelled = || gate.cancelled.get();
    let summary = rt_core::runner::run(
        &prepared.collection,
        &plan,
        &input,
        &files,
        &gate,
        &options,
        &cli.services(),
        &mut on_event,
        &cancelled,
    )
    .await;
    let mut steps = steps.into_inner();
    if !prepared.trusted && !server.opts.no_scripts {
        for step in &mut steps {
            rt_app::scripts::note_untrusted(&mut step.report, &prepared.collection);
        }
    }
    // The audit entry: one per call, with a line per request.
    let approvals = gate.approvals.into_inner();
    let sent = steps.iter().filter(|s| s.report.response.is_some()).count();
    entry.outcome = if sent > 0 {
        AuditOutcome::Sent
    } else if approvals
        .iter()
        .any(|(_, o)| matches!(o, Outcome::Denied(_)))
    {
        AuditOutcome::Denied
    } else if approvals.iter().any(|(_, o)| *o == Outcome::TimedOut) {
        AuditOutcome::TimedOut
    } else {
        AuditOutcome::Failed
    };
    entry.approved_by = approvals.iter().find_map(|(_, o)| match o {
        Outcome::Approved(by) => Some(by.clone()),
        _ => None,
    });
    entry.detail = steps
        .iter()
        .map(|s| {
            let what = match (&s.report.response, &s.report.error) {
                _ if s.skipped => "skipped".to_owned(),
                (Some(r), _) => r.status.to_string(),
                (None, Some(e)) => error_text(e),
                (None, None) => "not sent".to_owned(),
            };
            let method = s
                .report
                .request
                .as_ref()
                .map_or(String::new(), |r| format!("{} ", r.method));
            format!("{}: {method}{what}", s.request_path)
        })
        .collect::<Vec<_>>()
        .join("; ");
    let steps_json: Vec<Value> = steps
        .iter()
        .map(|s| {
            json!({
                "iteration": s.iteration,
                "request_path": s.request_path,
                "request_name": s.request_name,
                "passed": s.passed,
                "skipped": s.skipped,
                "report": s.report,
            })
        })
        .collect();
    let mut value = json!({"summary": summary, "steps": steps_json});
    server.masker.borrow().value(&mut value);
    Ok(ToolResult {
        is_error: summary.failed > 0,
        text: value.to_string(),
    })
}

fn has_folder(items: &[Item], path: &str) -> bool {
    items.iter().any(|item| match item {
        Item::Folder(f) => f.path == path || has_folder(&f.items, path),
        Item::Request(_) => false,
    })
}
