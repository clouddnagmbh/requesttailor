// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Testing MCP servers like an API (wave 8): connect, list tools, call one, save the call as
//! a regression test and run saved tests.
//!
//! - Transports: `stdio` (start a local command; desktop and CLI only, not the web) and
//!   `http` (Streamable HTTP; the web app sends through `fetch` or the companion).
//! - Saved tests are companion files of the collection, `<name>.mcp.yaml` (the OpenCollection
//!   reader reads only `.yml`, so they never show up as requests). `{{variables}}` in the
//!   server config and the arguments are resolved from the selected environment; secrets are
//!   referenced by name and never written into the file.
//! - Results are masked like reports.
//!
//! Owner: wave 8 interface. Implementation: strand D (file format in
//! `rt-format/src/mcp_test.rs`).
//!
//! # The client
//!
//! [`McpClient::connect`] starts the server (stdio) or posts to it (HTTP), sends `initialize`
//! (protocol version [`PROTOCOL_VERSION`], accepting the [`SUPPORTED_VERSIONS`]) and
//! `notifications/initialized`. [`McpClient::tools`] follows `nextCursor`;
//! [`McpClient::call`] times `tools/call`. Every value the client returns, errors included,
//! is masked with the secret values of its [`McpVars`]. The transport is passed per call, so
//! one client works with the native stack, `fetch` or the companion.
//!
//! - **Streamable HTTP** (`http`): every message is a `POST` with
//!   `Accept: application/json, text/event-stream`; the answer is JSON or an SSE stream, read
//!   until the response with the request's id. `Mcp-Session-Id` from `initialize` and the
//!   negotiated `MCP-Protocol-Version` go with every later request; a 404 with a session is
//!   "session expired". [`McpClient::disconnect`] sends `DELETE` with the session.
//! - **stdio** (native only): newline-delimited JSON-RPC on the process's stdin/stdout. The
//!   working directory is the collection root (or `cwd` below it); `env` comes on top of the
//!   inherited environment. Requests the server sends (`ping`, …) are answered; the last
//!   4 KiB of stderr go into error messages. Disconnecting closes stdin, then kills the
//!   process if it has not exited after two seconds. The calls block the thread while they
//!   wait (shells run them on a blocking thread, as sends).
//!
//! # Saved tests
//!
//! [`tests`], [`get_test`] and [`save_test`] work on the [`Workspace`]'s companion files, so
//! a saved test is staged like a request edit and written by `save`. [`run_test`] connects,
//! calls and evaluates the assertions with `rt_core::assert` against [`test_response`] (see
//! [`McpTest::assertions`]).

use std::path::Path;
use std::time::Duration;

use rt_core::model::{Assertion, Collection};
use rt_core::script::{AssertionResult, ScriptResponse};
use rt_core::send::MASK;
use rt_core::transport::Transport;
use rt_core::vars::{Scope, VarLayer, VarSource};
use rt_format::mcp_test::{ServerFile, TestFile};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AppError, Workspace};

mod http;
#[cfg(not(target_arch = "wasm32"))]
mod stdio;

/// File extension of saved MCP tests.
pub const TEST_EXTENSION: &str = "mcp.yaml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum McpServerConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        /// Extra environment variables for the process; values may be `{{templates}}`.
        env: Vec<(String, String)>,
        /// Working directory, relative to the collection root; `None`: the root.
        cwd: Option<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// What `initialize` told us.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
    pub protocol_version: String,
    /// Capability names the server announced (`tools`, `resources`, `prompts`, …).
    pub capabilities: Vec<String>,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// JSON Schema of the arguments.
    pub input_schema: serde_json::Value,
    pub output_schema: Option<serde_json::Value>,
    /// `annotations` as sent (`readOnlyHint`, `destructiveHint`, …).
    pub annotations: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum McpContent {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        data: String,
    },
    Audio {
        mime_type: String,
        data: String,
    },
    Resource {
        uri: String,
        text: Option<String>,
    },
    /// Anything else, as sent.
    Other {
        value: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCallResult {
    pub content: Vec<McpContent>,
    pub structured: Option<serde_json::Value>,
    pub is_error: bool,
    pub time_ms: u64,
}

/// A saved call: one `<name>.mcp.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTest {
    pub name: String,
    pub server: McpServerConfig,
    pub tool: String,
    pub arguments: serde_json::Value,
    /// Evaluated against the result as a response: `res.body` is `structured` if present,
    /// else the JSON of the first text content (or the text); `res.status` is 200, or 500
    /// for `is_error`.
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTestResult {
    /// Path of the test file relative to the collection root.
    pub path: String,
    pub passed: bool,
    pub result: Option<McpCallResult>,
    /// Connecting or calling failed (masked).
    pub error: Option<String>,
    pub assertions: Vec<AssertionResult>,
}

// ---------------------------------------------------------------------------------------
// Variables and masking

/// The protocol version the client asks for.
pub const PROTOCOL_VERSION: &str = "2025-11-25";
/// Versions the client accepts from a server, newest first.
pub const SUPPORTED_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// How long one request may take by default.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// The variables a connection resolves `{{templates}}` with, and the secret values it masks:
/// collection variables, the selected environment (with its secret values, as the shells
/// inject them for a send) and runtime variables, in that order of precedence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpVars {
    scope: Scope,
    /// Longest first, so a secret containing another is masked whole.
    secrets: Vec<String>,
}

impl McpVars {
    pub fn new(
        collection: &Collection,
        environment: Option<&str>,
        runtime: &[(String, String)],
    ) -> Self {
        let layer = |source: VarSource, vars: &[rt_core::model::Variable]| VarLayer {
            source,
            vars: vars
                .iter()
                .filter(|v| v.enabled)
                .filter_map(|v| Some((v.name.clone(), v.value.clone()?, v.secret)))
                .collect(),
        };
        let mut layers = vec![layer(VarSource::Collection, &collection.defaults.vars)];
        if let Some(env) = environment.and_then(|name| collection.environment(name)) {
            layers.push(layer(
                VarSource::Environment {
                    name: env.name.clone(),
                },
                &env.variables,
            ));
        }
        layers.push(VarLayer {
            source: VarSource::Runtime,
            vars: runtime
                .iter()
                .map(|(n, v)| (n.clone(), v.clone(), false))
                .collect(),
        });
        let scope = Scope { layers };
        // As for a send: a value is secret when its variable is secret in any layer or
        // declared secret in any environment (a `--var` given for a secret, in a pipeline).
        let declared: Vec<&str> = collection
            .defaults
            .vars
            .iter()
            .chain(collection.environments.iter().flat_map(|e| &e.variables))
            .filter(|v| v.secret)
            .map(|v| v.name.as_str())
            .collect();
        let mut secrets: Vec<String> = scope
            .layers
            .iter()
            .flat_map(|l| &l.vars)
            .filter(|(name, value, secret)| {
                (*secret || declared.contains(&name.as_str())) && !value.is_empty()
            })
            .map(|(_, value, _)| value.clone())
            .collect();
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        secrets.dedup();
        Self { scope, secrets }
    }

    /// From a run snapshot (the collection with the secret values of the environment).
    pub fn of_snapshot(snapshot: &crate::runner::RunSnapshot) -> Self {
        Self::new(
            &snapshot.collection,
            snapshot.environment.as_deref(),
            &snapshot.vars,
        )
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// Replaces `{{name}}` with its value; unknown names stay as written.
    pub fn resolve(&self, template: &str) -> String {
        if template.contains("{{") {
            self.scope.interpolate(template).value
        } else {
            template.to_owned()
        }
    }

    /// The server config with every text resolved.
    pub fn resolve_server(&self, server: &McpServerConfig) -> McpServerConfig {
        let pairs = |list: &[(String, String)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(n, v)| (self.resolve(n), self.resolve(v)))
                .collect()
        };
        match server {
            McpServerConfig::Stdio {
                command,
                args,
                env,
                cwd,
            } => McpServerConfig::Stdio {
                command: self.resolve(command),
                args: args.iter().map(|a| self.resolve(a)).collect(),
                env: pairs(env),
                cwd: cwd.as_deref().map(|c| self.resolve(c)),
            },
            McpServerConfig::Http { url, headers } => McpServerConfig::Http {
                url: self.resolve(url),
                headers: pairs(headers),
            },
        }
    }

    /// Arguments with every string (not the keys) resolved.
    pub fn resolve_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.resolve(s)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.resolve_value(v)).collect())
            }
            Value::Object(m) => Value::Object(
                m.iter()
                    .map(|(k, v)| (k.clone(), self.resolve_value(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Replaces every secret value, raw or percent-encoded, with `***`.
    pub fn mask(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for secret in &self.secrets {
            for form in [secret.clone(), percent_encode(secret)] {
                if !form.is_empty() && out.contains(&form) {
                    out = out.replace(&form, MASK);
                }
            }
        }
        out
    }

    /// [`mask`](Self::mask) on every string and key of a JSON value.
    pub fn mask_value(&self, value: Value) -> Value {
        if self.secrets.is_empty() {
            return value;
        }
        match value {
            Value::String(s) => Value::String(self.mask(&s)),
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|v| self.mask_value(v)).collect())
            }
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .map(|(k, v)| (self.mask(&k), self.mask_value(v)))
                    .collect(),
            ),
            other => other,
        }
    }

    fn mask_error(&self, e: McpError) -> McpError {
        match e {
            McpError::Connect(m) => McpError::Connect(self.mask(&m)),
            McpError::Http { status, body } => McpError::Http {
                status,
                body: self.mask(&body),
            },
            McpError::Rpc { code, message } => McpError::Rpc {
                code,
                message: self.mask(&message),
            },
            McpError::Protocol(m) => McpError::Protocol(self.mask(&m)),
            McpError::Closed(m) => McpError::Closed(self.mask(&m)),
            other => other,
        }
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Errors

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpError {
    /// The server could not be reached or started.
    #[error("cannot connect: {0}")]
    Connect(String),
    /// The HTTP endpoint answered with an error status.
    #[error("the server answered HTTP {status}{}", http_detail(body))]
    Http { status: u16, body: String },
    /// A JSON-RPC error.
    #[error("{message} (JSON-RPC error {code})")]
    Rpc { code: i64, message: String },
    /// The server's answer is not valid MCP.
    #[error("invalid answer from the server: {0}")]
    Protocol(String),
    #[error("the server did not answer in time")]
    Timeout,
    /// The HTTP session ended on the server (404 with `Mcp-Session-Id`).
    #[error("the server ended the session; connect again")]
    SessionExpired,
    /// The stdio server exited; the detail says how (exit status, stderr).
    #[error("the server process ended{0}")]
    Closed(String),
    /// Not possible in this shell (stdio in the browser).
    #[error("{0}")]
    Unsupported(String),
}

fn http_detail(body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else {
        format!(": {body}")
    }
}

impl From<McpError> for AppError {
    fn from(e: McpError) -> Self {
        match e {
            McpError::Unsupported(m) => AppError::Unsupported(m),
            other => AppError::Shell(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------------------
// The client

enum Wire {
    Http(http::HttpWire),
    #[cfg(not(target_arch = "wasm32"))]
    Stdio(stdio::StdioWire),
}

/// One connection to an MCP server. HTTP calls take the transport per call.
pub struct McpClient {
    wire: Wire,
    info: McpServerInfo,
    vars: McpVars,
    next_id: u64,
    timeout: Duration,
    /// What the server is (URL or command, masked), for test responses.
    target: String,
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl McpClient {
    /// Resolves `server` with `vars`, connects and initializes. `root`: the collection's
    /// folder, the working directory of stdio servers (`None`: the current one).
    pub async fn connect<T: Transport>(
        transport: &T,
        server: &McpServerConfig,
        vars: McpVars,
        root: Option<&Path>,
    ) -> Result<Self, McpError> {
        Self::connect_with(transport, server, vars, root, DEFAULT_TIMEOUT).await
    }

    /// [`connect`](Self::connect) with another time limit per request.
    pub async fn connect_with<T: Transport>(
        transport: &T,
        server: &McpServerConfig,
        vars: McpVars,
        root: Option<&Path>,
        timeout: Duration,
    ) -> Result<Self, McpError> {
        let resolved = vars.resolve_server(server);
        let (wire, target) = match &resolved {
            McpServerConfig::Http { url, headers } => (
                Wire::Http(http::HttpWire::new(url.clone(), headers.clone())),
                url.clone(),
            ),
            #[cfg(not(target_arch = "wasm32"))]
            McpServerConfig::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                let wire = stdio::StdioWire::spawn(command, args, env, cwd.as_deref(), root)
                    .map_err(|e| vars.mask_error(e))?;
                (Wire::Stdio(wire), command.clone())
            }
            #[cfg(target_arch = "wasm32")]
            McpServerConfig::Stdio { .. } => {
                let _ = root;
                return Err(McpError::Unsupported(
                    "stdio MCP servers start a local command, which the browser cannot do: use the desktop app or `rt mcp-test`, or connect over HTTP".into(),
                ));
            }
        };
        let target = vars.mask(&target);
        let mut client = Self {
            wire,
            info: McpServerInfo::default(),
            vars,
            next_id: 1,
            timeout,
            target,
        };
        let result = client
            .request(
                transport,
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "RequestTailor", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;
        let result = client.vars.mask_value(result);
        let info = server_info(&result)?;
        // stdio has no headers; only HTTP sends the version with every request.
        #[cfg_attr(target_arch = "wasm32", allow(irrefutable_let_patterns))]
        if let Wire::Http(w) = &mut client.wire {
            w.protocol = Some(info.protocol_version.clone());
        }
        client
            .notify(transport, "notifications/initialized")
            .await?;
        client.info = info;
        Ok(client)
    }

    /// What `initialize` told us (masked).
    pub fn info(&self) -> &McpServerInfo {
        &self.info
    }

    /// The server's URL or command (resolved, masked).
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Every tool, following `nextCursor` (at most 100 pages). Masked.
    pub async fn tools<T: Transport>(&mut self, transport: &T) -> Result<Vec<McpTool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..100 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let result = self.request(transport, "tools/list", params).await?;
            let result = self.vars.mask_value(result);
            let list = result
                .get("tools")
                .and_then(Value::as_array)
                .ok_or_else(|| McpError::Protocol("tools/list without tools".into()))?;
            for t in list {
                tools.push(tool_of(t)?);
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
                .map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// Calls `tool` with `arguments` (`{{variables}}` resolved). A tool that fails reports
    /// `is_error`; a JSON-RPC error (unknown tool, invalid arguments) is an `Err`. Masked.
    pub async fn call<T: Transport>(
        &mut self,
        transport: &T,
        tool: &str,
        arguments: &Value,
    ) -> Result<McpCallResult, McpError> {
        let result = self.call_unmasked(transport, tool, arguments).await?;
        Ok(mask_result(&self.vars, result))
    }

    /// [`call`](Self::call) without masking the result (errors are masked): assertions of
    /// saved tests see the real values, as they see real responses.
    async fn call_unmasked<T: Transport>(
        &mut self,
        transport: &T,
        tool: &str,
        arguments: &Value,
    ) -> Result<McpCallResult, McpError> {
        let arguments = self.vars.resolve_value(arguments);
        let started = web_time::Instant::now();
        let result = self
            .request(
                transport,
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
            )
            .await?;
        let time_ms = started.elapsed().as_millis() as u64;
        Ok(call_result(result, time_ms))
    }

    /// Ends the session (HTTP `DELETE`) or the process (stdio). Errors are ignored.
    pub async fn disconnect<T: Transport>(self, transport: &T) {
        match self.wire {
            Wire::Http(w) => w.close(transport, self.timeout).await,
            #[cfg(not(target_arch = "wasm32"))]
            Wire::Stdio(w) => w.close(),
        }
    }

    async fn request<T: Transport>(
        &mut self,
        transport: &T,
        method: &str,
        params: Value,
    ) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let answer = match &mut self.wire {
            Wire::Http(w) => {
                w.exchange(transport, &message, Some(id), self.timeout)
                    .await
            }
            #[cfg(not(target_arch = "wasm32"))]
            Wire::Stdio(w) => w.exchange(&message, Some(id), self.timeout),
        }
        .map_err(|e| self.vars.mask_error(e))?
        .ok_or_else(|| McpError::Protocol(format!("no answer to {method}")))?;
        if let Some(error) = answer.get("error") {
            return Err(self.vars.mask_error(McpError::Rpc {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
                    .to_owned(),
            }));
        }
        answer
            .get("result")
            .cloned()
            .ok_or_else(|| McpError::Protocol(format!("{method}: answer without result")))
    }

    async fn notify<T: Transport>(&mut self, transport: &T, method: &str) -> Result<(), McpError> {
        let message = json!({ "jsonrpc": "2.0", "method": method });
        match &mut self.wire {
            Wire::Http(w) => w.exchange(transport, &message, None, self.timeout).await,
            #[cfg(not(target_arch = "wasm32"))]
            Wire::Stdio(w) => w.exchange(&message, None, self.timeout),
        }
        .map(|_| ())
        .map_err(|e| self.vars.mask_error(e))
    }
}

fn server_info(result: &Value) -> Result<McpServerInfo, McpError> {
    let version = result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::Protocol("initialize without protocolVersion".into()))?;
    if !SUPPORTED_VERSIONS.contains(&version) {
        return Err(McpError::Protocol(format!(
            "the server speaks MCP {version}; supported are {}",
            SUPPORTED_VERSIONS.join(", ")
        )));
    }
    let text = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").to_owned();
    let mut capabilities: Vec<String> = result
        .get("capabilities")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    capabilities.sort();
    Ok(McpServerInfo {
        name: text(result.pointer("/serverInfo/name")),
        version: text(result.pointer("/serverInfo/version")),
        protocol_version: version.to_owned(),
        capabilities,
        instructions: result
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn tool_of(t: &Value) -> Result<McpTool, McpError> {
    let name = t
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::Protocol("a tool without a name".into()))?;
    let text = |key: &str| t.get(key).and_then(Value::as_str).map(str::to_owned);
    let annotations = t.get("annotations").filter(|a| a.is_object()).cloned();
    Ok(McpTool {
        name: name.to_owned(),
        title: text("title").or_else(|| {
            annotations
                .as_ref()
                .and_then(|a| a.get("title"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        }),
        description: text("description"),
        input_schema: t
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object" })),
        output_schema: t.get("outputSchema").filter(|s| !s.is_null()).cloned(),
        annotations,
    })
}

fn mask_result(vars: &McpVars, result: McpCallResult) -> McpCallResult {
    let content = result
        .content
        .into_iter()
        .map(|c| match c {
            McpContent::Text { text } => McpContent::Text {
                text: vars.mask(&text),
            },
            McpContent::Image { mime_type, data } => McpContent::Image {
                mime_type: vars.mask(&mime_type),
                data: vars.mask(&data),
            },
            McpContent::Audio { mime_type, data } => McpContent::Audio {
                mime_type: vars.mask(&mime_type),
                data: vars.mask(&data),
            },
            McpContent::Resource { uri, text } => McpContent::Resource {
                uri: vars.mask(&uri),
                text: text.map(|t| vars.mask(&t)),
            },
            McpContent::Other { value } => McpContent::Other {
                value: vars.mask_value(value),
            },
        })
        .collect();
    McpCallResult {
        content,
        structured: result.structured.map(|v| vars.mask_value(v)),
        ..result
    }
}

fn call_result(result: Value, time_ms: u64) -> McpCallResult {
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|c| match c.get("type").and_then(Value::as_str) {
                    Some("text") if c.get("text").is_some_and(Value::is_string) => {
                        McpContent::Text {
                            text: text(c, "text").unwrap_or_default(),
                        }
                    }
                    Some("image") => McpContent::Image {
                        mime_type: text(c, "mimeType").unwrap_or_default(),
                        data: text(c, "data").unwrap_or_default(),
                    },
                    Some("audio") => McpContent::Audio {
                        mime_type: text(c, "mimeType").unwrap_or_default(),
                        data: text(c, "data").unwrap_or_default(),
                    },
                    Some("resource") if c.get("resource").is_some_and(Value::is_object) => {
                        let r = &c["resource"];
                        McpContent::Resource {
                            uri: text(r, "uri").unwrap_or_default(),
                            text: text(r, "text"),
                        }
                    }
                    _ => McpContent::Other { value: c.clone() },
                })
                .collect()
        })
        .unwrap_or_default();
    McpCallResult {
        content,
        structured: result
            .get("structuredContent")
            .filter(|v| !v.is_null())
            .cloned(),
        is_error: result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        time_ms,
    }
}

// ---------------------------------------------------------------------------------------
// Saved tests

/// The response assertions of a test see: `res.status` 200 (500 for `is_error`),
/// `res.body` the structured result if present, else the first text content (JSON if it
/// parses, else text), `res.responseTime` the call's time; `url` the server (`target`).
pub fn test_response(result: &McpCallResult, target: &str) -> ScriptResponse {
    let first_text = result.content.iter().find_map(|c| match c {
        McpContent::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let (body, json) = match (&result.structured, first_text) {
        (Some(v), _) => (v.to_string().into_bytes(), true),
        (None, Some(t)) => (
            t.as_bytes().to_vec(),
            serde_json::from_str::<Value>(t).is_ok(),
        ),
        (None, None) => (b"null".to_vec(), true),
    };
    ScriptResponse {
        status: if result.is_error { 500 } else { 200 },
        headers: vec![(
            "content-type".into(),
            if json {
                "application/json"
            } else {
                "text/plain"
            }
            .into(),
        )],
        body,
        time_ms: result.time_ms,
        url: target.to_owned(),
    }
}

/// Saved tests (`*.mcp.yaml`) of the workspace, staged ones included, sorted.
pub fn tests(workspace: &Workspace) -> Vec<String> {
    workspace.companion_paths(rt_format::mcp_test::is_test_path)
}

/// A saved test (with unsaved edits).
pub fn get_test(workspace: &Workspace, path: &str) -> Result<McpTest, AppError> {
    let bytes = workspace
        .companion_file(path)
        .filter(|_| rt_format::mcp_test::is_test_path(path))
        .ok_or_else(|| AppError::Shell(format!("no MCP test {path}")))?;
    parse_test(path, bytes)
}

/// Reads a test file (`path` for messages).
pub fn parse_test(path: &str, bytes: &[u8]) -> Result<McpTest, AppError> {
    Ok(from_file(rt_format::mcp_test::read(path, bytes)?))
}

/// Renders a test; an unchanged one keeps `previous` byte for byte.
pub fn render_test(test: &McpTest, previous: Option<&[u8]>) -> String {
    rt_format::mcp_test::write(&to_file(test), previous)
}

/// Stages `test` as a file of the workspace (written by `save`, like a request edit).
/// `path` `None`: a new file `mcp/<name>.mcp.yaml` (made unique). Refused with
/// `SecretsFound` when the file would contain one of `secret_values` (the shell passes the
/// stored values of the collection's secret variables) or what the secret scanner calls a
/// credential: secrets belong in variables, referenced as `{{name}}`. Returns the path.
pub fn save_test(
    workspace: &mut Workspace,
    path: Option<&str>,
    test: &McpTest,
    secret_values: &[String],
) -> Result<String, AppError> {
    let path = match path {
        Some(p) if rt_format::mcp_test::is_test_path(p) => p.to_owned(),
        Some(p) => {
            return Err(AppError::Shell(format!(
                "{p}: MCP tests are files ending in .{TEST_EXTENSION}"
            )));
        }
        None => new_test_path(workspace, &test.name),
    };
    let text = render_test(test, workspace.companion_file(&path));
    if secret_values
        .iter()
        .any(|v| v.len() >= 3 && text.contains(v.as_str()))
    {
        return Err(AppError::SecretsFound(format!(
            "Not saved: {path} would contain the value of a secret variable. Write {{{{name}}}} instead of the value."
        )));
    }
    let findings = crate::secret_scan::scan(&[(&path, &text)]);
    if let Some(f) = findings.first() {
        return Err(AppError::SecretsFound(format!(
            "Not saved: {} on line {} of {path} ({}). {}",
            f.kind, f.line, f.snippet, f.hint
        )));
    }
    workspace.stage_companion(&path, text.into_bytes())?;
    Ok(path)
}

fn new_test_path(workspace: &Workspace, name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_end_matches('-').chars().take(64).collect();
    let slug = if slug.is_empty() {
        "test".to_owned()
    } else {
        slug
    };
    let taken: Vec<String> = workspace
        .companion_paths(|_| true)
        .into_iter()
        .map(|p| p.to_lowercase())
        .collect();
    (1..)
        .map(|n| match n {
            1 => format!("mcp/{slug}.{TEST_EXTENSION}"),
            n => format!("mcp/{slug}-{n}.{TEST_EXTENSION}"),
        })
        .find(|candidate| !taken.contains(&candidate.to_lowercase()))
        .expect("a free name")
}

/// Runs one saved test: connect, call, assertions, disconnect. Never fails: a connection or
/// call error is in `error` (masked) and fails the test. Without assertions a test passes
/// when the call is not `is_error`.
pub async fn run_test<T: Transport>(
    transport: &T,
    path: &str,
    test: &McpTest,
    vars: &McpVars,
    root: Option<&Path>,
) -> McpTestResult {
    run_test_outcome(transport, path, test, vars, root)
        .await
        .result
}

/// What [`run_test_outcome`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct TestOutcome {
    pub result: McpTestResult,
    /// The server could not be reached or started (`rt mcp-test` exits 1 then).
    pub connect_failed: bool,
}

/// [`run_test`], and whether connecting failed.
pub async fn run_test_outcome<T: Transport>(
    transport: &T,
    path: &str,
    test: &McpTest,
    vars: &McpVars,
    root: Option<&Path>,
) -> TestOutcome {
    let failed = |error: McpError| McpTestResult {
        path: path.to_owned(),
        passed: false,
        result: None,
        error: Some(error.to_string()),
        assertions: Vec::new(),
    };
    let client = match McpClient::connect(transport, &test.server, vars.clone(), root).await {
        Ok(c) => c,
        Err(e) => {
            return TestOutcome {
                result: failed(e),
                connect_failed: true,
            };
        }
    };
    TestOutcome {
        result: call_and_check(transport, client, path, test, vars).await,
        connect_failed: false,
    }
}

async fn call_and_check<T: Transport>(
    transport: &T,
    mut client: McpClient,
    path: &str,
    test: &McpTest,
    vars: &McpVars,
) -> McpTestResult {
    let failed = |error: McpError| McpTestResult {
        path: path.to_owned(),
        passed: false,
        result: None,
        error: Some(error.to_string()),
        assertions: Vec::new(),
    };
    let outcome = client
        .call_unmasked(transport, &test.tool, &test.arguments)
        .await;
    let target = client.target.clone();
    client.disconnect(transport).await;
    let result = match outcome {
        Ok(r) => r,
        Err(e) => return failed(e),
    };
    let assertions = evaluate(&test.assertions, &result, &target, vars);
    let passed = if assertions.is_empty() {
        !result.is_error
    } else {
        assertions.iter().all(|a| a.passed)
    };
    McpTestResult {
        path: path.to_owned(),
        passed,
        result: Some(mask_result(vars, result)),
        error: None,
        assertions,
    }
}

/// The enabled assertions against a result; what they report is masked.
pub fn evaluate(
    assertions: &[Assertion],
    result: &McpCallResult,
    target: &str,
    vars: &McpVars,
) -> Vec<AssertionResult> {
    rt_core::assert::evaluate(assertions, &test_response(result, target), vars.scope())
        .into_iter()
        .map(|a| AssertionResult {
            expected: vars.mask(&a.expected),
            actual: a.actual.map(|v| vars.mask(&v)),
            error: a.error.map(|v| vars.mask(&v)),
            ..a
        })
        .collect()
}

fn to_file(test: &McpTest) -> TestFile {
    TestFile {
        name: test.name.clone(),
        server: match &test.server {
            McpServerConfig::Stdio {
                command,
                args,
                env,
                cwd,
            } => ServerFile::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
                cwd: cwd.clone(),
            },
            McpServerConfig::Http { url, headers } => ServerFile::Http {
                url: url.clone(),
                headers: headers.clone(),
            },
        },
        tool: test.tool.clone(),
        arguments: test.arguments.clone(),
        assertions: test.assertions.clone(),
    }
}

fn from_file(file: TestFile) -> McpTest {
    McpTest {
        name: file.name,
        server: match file.server {
            ServerFile::Stdio {
                command,
                args,
                env,
                cwd,
            } => McpServerConfig::Stdio {
                command,
                args,
                env,
                cwd,
            },
            ServerFile::Http { url, headers } => McpServerConfig::Http { url, headers },
        },
        tool: file.tool,
        arguments: file.arguments,
        assertions: file.assertions,
    }
}

#[cfg(test)]
mod tests {
    use rt_core::model::{Environment, Variable};

    use super::*;

    fn vars() -> McpVars {
        let mut collection = Collection::default();
        collection.defaults.vars.push(Variable::new("tool", "echo"));
        collection.environments.push(Environment {
            name: "Local".into(),
            variables: vec![
                Variable::new("url", "http://127.0.0.1:1/mcp"),
                Variable {
                    name: "token".into(),
                    value: Some("s3cr3t value".into()),
                    enabled: true,
                    secret: true,
                },
            ],
            client_certificates: Vec::new(),
            external_secrets: None,
        });
        McpVars::new(&collection, Some("Local"), &[("tool".into(), "add".into())])
    }

    #[test]
    fn resolves_and_masks() {
        let v = vars();
        assert_eq!(
            v.resolve("{{tool}} at {{url}}"),
            "add at http://127.0.0.1:1/mcp"
        );
        assert_eq!(v.resolve("{{missing}}"), "{{missing}}");
        let server = v.resolve_server(&McpServerConfig::Http {
            url: "{{url}}".into(),
            headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
        });
        assert_eq!(
            server,
            McpServerConfig::Http {
                url: "http://127.0.0.1:1/mcp".into(),
                headers: vec![("Authorization".into(), "Bearer s3cr3t value".into())],
            }
        );
        assert_eq!(
            v.resolve_value(&json!({"a": ["{{tool}}", 1], "{{tool}}": "x"})),
            json!({"a": ["add", 1], "{{tool}}": "x"})
        );
        assert_eq!(
            v.mask("Bearer s3cr3t value / s3cr3t%20value"),
            "Bearer *** / ***"
        );
        assert_eq!(
            v.mask_value(json!({"s3cr3t value": ["s3cr3t value"]})),
            json!({"***": ["***"]})
        );

        // A runtime value for a variable declared secret is masked too (`--var token=...`).
        let mut collection = Collection::default();
        collection.environments.push(Environment {
            name: "Local".into(),
            variables: vec![Variable {
                name: "token".into(),
                value: None,
                enabled: true,
                secret: true,
            }],
            client_certificates: Vec::new(),
            external_secrets: None,
        });
        let v = McpVars::new(
            &collection,
            Some("Local"),
            &[("token".into(), "pipe-tok".into())],
        );
        assert_eq!(v.resolve("{{token}}"), "pipe-tok");
        assert_eq!(v.mask("x pipe-tok"), "x ***");
    }

    #[test]
    fn test_responses_follow_the_documented_rules() {
        let text = |t: &str| McpContent::Text { text: t.into() };
        let result = |content, structured, is_error| McpCallResult {
            content,
            structured,
            is_error,
            time_ms: 7,
        };
        let r = test_response(
            &result(vec![text("{\"a\":1}")], Some(json!({"b": 2})), false),
            "u",
        );
        assert_eq!(
            (r.status, r.body.as_slice()),
            (200, br#"{"b":2}"#.as_slice())
        );
        let r = test_response(&result(vec![text("{\"a\":1}")], None, true), "u");
        assert_eq!(
            (r.status, r.body.as_slice()),
            (500, br#"{"a":1}"#.as_slice())
        );
        let r = test_response(&result(vec![text("plain")], None, false), "u");
        assert_eq!(r.headers[0].1, "text/plain");
        assert_eq!(r.time_ms, 7);

        let a = |e: &str, op: &str, v: &str| Assertion {
            expression: e.into(),
            operator: op.into(),
            value: v.into(),
            description: None,
            enabled: true,
        };
        let results = evaluate(
            &[
                a("res.body.b", "eq", "2"),
                a("res.status", "eq", "200"),
                a("res.body.b", "eq", "{{token}}"),
            ],
            &result(vec![], Some(json!({"b": 2})), false),
            "u",
            &vars(),
        );
        assert!(results[0].passed && results[1].passed && !results[2].passed);
        assert_eq!(results[2].expected, "***");
    }

    #[test]
    fn tests_are_staged_in_the_workspace() {
        let mut files = rt_format::FileSet::default();
        files.files.insert(
            "opencollection.yml".into(),
            b"opencollection: 1.0.0\ninfo:\n  name: T\n".to_vec(),
        );
        files.files.insert(
            "mcp/old.mcp.yaml".into(),
            b"name: old\nserver:\n  transport: http\n  url: u\ntool: echo\n".to_vec(),
        );
        let mut ws = Workspace::open(files).unwrap();
        assert_eq!(tests(&ws), ["mcp/old.mcp.yaml"]);
        assert!(ws.dirty().is_empty());
        let test = McpTest {
            name: "Echo: hi!".into(),
            server: McpServerConfig::Http {
                url: "{{url}}".into(),
                headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
            },
            tool: "echo".into(),
            arguments: json!({"text": "hi"}),
            assertions: Vec::new(),
        };
        let path = save_test(&mut ws, None, &test, &[]).unwrap();
        assert_eq!(path, "mcp/echo-hi.mcp.yaml");
        assert_eq!(
            save_test(&mut ws, None, &test, &[]).unwrap(),
            "mcp/echo-hi-2.mcp.yaml"
        );
        assert_eq!(tests(&ws).len(), 3);
        assert_eq!(get_test(&ws, &path).unwrap(), test);
        assert_eq!(
            ws.dirty(),
            ["mcp/echo-hi-2.mcp.yaml", "mcp/echo-hi.mcp.yaml"]
        );

        // Secret values and literal credentials are refused.
        let mut leaky = test.clone();
        leaky.arguments = json!({"text": "s3cr3t value"});
        let err = save_test(&mut ws, Some(&path), &leaky, &["s3cr3t value".into()]).unwrap_err();
        assert!(matches!(err, AppError::SecretsFound(_)), "{err:?}");
        let mut literal = test.clone();
        literal.server = McpServerConfig::Http {
            url: "{{url}}".into(),
            headers: vec![(
                "Authorization".into(),
                "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U".into(),
            )],
        };
        let err = save_test(&mut ws, Some(&path), &literal, &[]).unwrap_err();
        assert!(matches!(err, AppError::SecretsFound(_)), "{err:?}");
        assert!(save_test(&mut ws, Some("mcp/x.yml"), &test, &[]).is_err());

        // Saved like request edits.
        let changes = ws.save().unwrap();
        let paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["mcp/echo-hi-2.mcp.yaml", "mcp/echo-hi.mcp.yaml"]);
        assert!(
            changes[1].text.contains("url: \"{{url}}\""),
            "{}",
            changes[1].text
        );
        assert!(ws.dirty().is_empty());
        assert_eq!(get_test(&ws, &path).unwrap(), test);
        // Staging the bytes on disk again is no change.
        save_test(&mut ws, Some(&path), &test, &[]).unwrap();
        assert!(ws.dirty().is_empty());
    }
}
