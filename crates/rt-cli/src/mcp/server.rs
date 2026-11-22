// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! JSON-RPC 2.0 over stdio for `rt mcp`: one message per line, protocol negotiation, the
//! message loop.
//!
//! - Supported protocol versions: [`PROTOCOL_VERSIONS`]. `initialize` answers with the
//!   client's version if it is one of them, else with the newest (the client then decides
//!   whether to go on, as the specification says).
//! - Methods: `initialize`, `ping`, `tools/list`, `tools/call`; notifications
//!   `notifications/initialized` and `notifications/cancelled`. Anything else is the JSON-RPC
//!   error "method not found"; a request before `initialize` is refused. Batches (arrays,
//!   protocol 2025-03-26) are answered as arrays.
//! - Calls run one at a time. While a call waits for an approval, the loop still answers
//!   `ping`, sees `notifications/cancelled` for that call and the answer to an elicitation;
//!   other requests wait their turn.
//! - Every outgoing line goes through [`Masker::line`]; stderr gets no secret values either.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use rt_app::agents::{self, AuditEntry, AuditOutcome};
use rt_core::model::AgentPolicy;
use rt_core::transport::native::NativeTransport;
use serde_json::{Value, json};

use super::Args;
use super::mask::Masker;

/// Newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// Before `initialize`.
pub const NOT_INITIALIZED: i64 = -32002;

/// A JSON-RPC error.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// What the stdin reader thread delivers.
pub enum Incoming {
    Line(String),
    Eof,
}

/// Reads stdin on a thread, line by line.
pub fn stdin_lines() -> Receiver<Incoming> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(Incoming::Line(line)).is_err() {
                return;
            }
        }
        let _ = tx.send(Incoming::Eof);
    });
    rx
}

/// What the client said in `initialize`.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub initialized: bool,
    pub client_name: Option<String>,
    pub protocol: String,
    /// The client announced `elicitation`.
    pub elicitation: bool,
}

struct Io<'a> {
    out: &'a mut dyn Write,
    rx: Option<Receiver<Incoming>>,
    /// Requests that arrived while a call was waiting.
    backlog: VecDeque<Value>,
    eof: bool,
    /// Ids of requests to the client (`rt-1`, `rt-2`, ...).
    next_id: u64,
}

/// What arrived while a call waited.
pub enum Polled {
    Nothing,
    /// The answer (whole message) to the request with the id asked for.
    Answer(Value),
    /// The call was cancelled, or the client went away.
    Cancelled,
}

pub struct Server<'a> {
    pub opts: Args,
    /// Canonical collection root.
    pub root: PathBuf,
    /// Collection id as the desktop app uses it (keychain, script trust).
    pub collection_id: String,
    pub data_dir: PathBuf,
    /// The policy as `rt mcp` started (see `policy`).
    pub pinned: AgentPolicy,
    pub masker: RefCell<Masker>,
    pub session: RefCell<Session>,
    pub transport: NativeTransport,
    io: RefCell<Io<'a>>,
    stderr: RefCell<&'a mut dyn Write>,
    runtime: tokio::runtime::Runtime,
}

/// The collection root for `path`: the nearest folder at or above it with
/// `opencollection.yml`.
fn find_collection(path: &Path) -> Result<PathBuf, String> {
    let canonical =
        std::fs::canonicalize(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    canonical
        .ancestors()
        .find(|dir| dir.join(rt_format::COLLECTION_FILE).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "no {} in {} or any folder above it",
                rt_format::COLLECTION_FILE,
                canonical.display()
            )
        })
}

impl<'a> Server<'a> {
    pub fn new(
        opts: Args,
        out: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
    ) -> Result<Self, String> {
        let root = find_collection(opts.collection.as_deref().unwrap_or(Path::new(".")))?;
        let doc = crate::load_collection(&root)?;
        let data_dir = rt_plugin::host::dir::data_dir()
            .ok_or("no data folder for approvals and the audit log (set RT_DATA_DIR)")?;
        let mut masker = Masker::default();
        for (_, value) in &opts.vars {
            masker.add(value);
        }
        for env in &doc.model().environments {
            for v in env.variables.iter().filter(|v| v.secret) {
                if let Some(value) = &v.value {
                    masker.add(value);
                }
            }
        }
        let transport = NativeTransport::new().map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("cannot start runtime: {e}"))?;
        Ok(Self {
            collection_id: crate::oauth::collection_id(&root),
            pinned: doc.model().config.agents.clone(),
            root,
            data_dir,
            opts,
            masker: RefCell::new(masker),
            session: RefCell::new(Session::default()),
            transport,
            io: RefCell::new(Io {
                out,
                rx: None,
                backlog: VecDeque::new(),
                eof: false,
                next_id: 1,
            }),
            stderr: RefCell::new(stderr),
            runtime,
        })
    }

    /// The agent's name for approvals and the audit log.
    pub fn agent(&self) -> String {
        self.session
            .borrow()
            .client_name
            .clone()
            .or_else(|| self.opts.agent_name.clone())
            .unwrap_or_else(|| "unknown agent".into())
    }

    /// A diagnostic on stderr (masked).
    pub fn log(&self, text: &str) {
        let text = self.masker.borrow().text(text);
        let mut err = self.stderr.borrow_mut();
        let _ = writeln!(err, "rt mcp: {text}");
    }

    /// Writes one message (masked).
    pub fn send(&self, message: &Value) {
        let line = self.masker.borrow().line(message);
        let mut io = self.io.borrow_mut();
        let _ = writeln!(io.out, "{line}");
        let _ = io.out.flush();
    }

    /// Sends a request to the client; returns its id.
    pub fn request_client(&self, method: &str, params: Value) -> String {
        let id = {
            let mut io = self.io.borrow_mut();
            let id = format!("rt-{}", io.next_id);
            io.next_id += 1;
            id
        };
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    /// Appends an audit entry, masking `url` and `detail`.
    pub fn audit(&self, mut entry: AuditEntry) {
        {
            let masker = self.masker.borrow();
            entry.url = entry.url.map(|u| masker.text(&u));
            entry.detail = masker.text(&entry.detail);
        }
        if let Err(e) = agents::append_audit(&self.data_dir, &entry) {
            self.log(&format!("cannot write the audit log: {e}"));
        }
    }

    /// An audit entry for this server with the common fields filled in.
    pub fn audit_entry(&self, tool: &str, outcome: AuditOutcome) -> AuditEntry {
        AuditEntry {
            at: agents::now_ms(),
            agent: self.agent(),
            tool: tool.to_owned(),
            collection: self.root.display().to_string(),
            request_path: None,
            environment: None,
            method: None,
            url: None,
            status: None,
            outcome,
            approved_by: None,
            detail: String::new(),
        }
    }

    /// Serves until the input ends.
    pub fn serve(&mut self, rx: Receiver<Incoming>) {
        self.io.get_mut().rx = Some(rx);
        loop {
            let next = self.io.borrow_mut().backlog.pop_front();
            let message = match next {
                Some(m) => m,
                None => {
                    if self.io.borrow().eof {
                        break;
                    }
                    let received = self.io.borrow().rx.as_ref().map(Receiver::recv);
                    match received {
                        Some(Ok(Incoming::Line(line))) => match parse_line(&line) {
                            Ok(Some(v)) => v,
                            Ok(None) => continue,
                            Err(e) => {
                                self.send(&error_response(&Value::Null, &e));
                                continue;
                            }
                        },
                        _ => break,
                    }
                }
            };
            if let Some(reply) = self.handle(message) {
                self.send(&reply);
            }
        }
    }

    /// Handles one message or batch; the reply, if any.
    pub fn handle(&self, message: Value) -> Option<Value> {
        match message {
            Value::Array(batch) if batch.is_empty() => Some(error_response(
                &Value::Null,
                &RpcError::new(INVALID_REQUEST, "empty batch"),
            )),
            Value::Array(batch) => {
                let replies: Vec<Value> = batch
                    .into_iter()
                    .filter_map(|m| self.handle_one(m))
                    .collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            m => self.handle_one(m),
        }
    }

    fn handle_one(&self, message: Value) -> Option<Value> {
        let Some(obj) = message.as_object() else {
            return Some(error_response(
                &Value::Null,
                &RpcError::new(INVALID_REQUEST, "a message is a JSON object"),
            ));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            // A response to a request of ours that nobody waits for any more.
            if id.is_some() && (obj.contains_key("result") || obj.contains_key("error")) {
                return None;
            }
            return Some(error_response(
                &id.unwrap_or(Value::Null),
                &RpcError::new(INVALID_REQUEST, "missing method"),
            ));
        };
        let params = obj.get("params").cloned().unwrap_or(json!({}));
        let Some(id) = id else {
            self.notification(method, &params);
            return None;
        };
        let result = self.dispatch(method, &id, &params);
        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => error_response(&id, &e),
        })
    }

    fn notification(&self, method: &str, _params: &Value) {
        if method == "notifications/initialized" {
            self.session.borrow_mut().initialized = true;
        }
        // `notifications/cancelled` outside a wait: the call already finished.
    }

    fn dispatch(&self, method: &str, id: &Value, params: &Value) -> Result<Value, RpcError> {
        let started = !self.session.borrow().protocol.is_empty();
        match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(json!({})),
            _ if !started => Err(RpcError::new(
                NOT_INITIALIZED,
                "the server is not initialized: send `initialize` first",
            )),
            "tools/list" => Ok(super::tools::list()),
            "tools/call" => self.runtime.block_on(super::tools::call(self, id, params)),
            _ => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("method not found: {method}"),
            )),
        }
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let protocol = if PROTOCOL_VERSIONS.contains(&asked) {
            asked
        } else {
            PROTOCOL_VERSIONS[0]
        };
        let client = params.get("clientInfo");
        let name = client
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| n.chars().take(80).collect::<String>());
        let elicitation = params
            .get("capabilities")
            .and_then(|c| c.get("elicitation"))
            .is_some_and(Value::is_object);
        {
            let mut s = self.session.borrow_mut();
            s.protocol = protocol.to_owned();
            s.client_name = name;
            s.elicitation = elicitation;
        }
        let version = client
            .and_then(|c| c.get("version"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        let mut entry = self.audit_entry("initialize", AuditOutcome::Read);
        entry.detail = format!(
            "connected (MCP {protocol}, client version {})",
            version.chars().take(40).collect::<String>()
        );
        self.audit(entry);
        json!({
            "protocolVersion": protocol,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {
                "name": "requesttailor",
                "title": "RequestTailor",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": INSTRUCTIONS,
        })
    }

    /// Collects what arrived while a call waits (see the module documentation). `call_id` is
    /// the waiting call, `answer_to` the id of a request of ours whose answer is awaited.
    pub fn poll(&self, call_id: &Value, answer_to: Option<&str>) -> Polled {
        loop {
            let received = {
                let io = self.io.borrow();
                match io.rx.as_ref() {
                    Some(rx) => rx.try_recv(),
                    None => Err(TryRecvError::Empty),
                }
            };
            let line = match received {
                Ok(Incoming::Line(line)) => line,
                Ok(Incoming::Eof) | Err(TryRecvError::Disconnected) => {
                    self.io.borrow_mut().eof = true;
                    return Polled::Cancelled;
                }
                Err(TryRecvError::Empty) => return Polled::Nothing,
            };
            let message = match parse_line(&line) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                Err(e) => {
                    self.send(&error_response(&Value::Null, &e));
                    continue;
                }
            };
            let method = message.get("method").and_then(Value::as_str);
            match method {
                None if answer_to.is_some()
                    && message.get("id").and_then(Value::as_str) == answer_to =>
                {
                    return Polled::Answer(message);
                }
                None => {}
                Some("ping") if message.get("id").is_some() => {
                    self.send(&json!({"jsonrpc": "2.0", "id": message["id"], "result": {}}));
                }
                Some("notifications/cancelled")
                    if message
                        .get("params")
                        .and_then(|p| p.get("requestId"))
                        .is_some_and(|r| r == call_id) =>
                {
                    return Polled::Cancelled;
                }
                Some(_) => self.io.borrow_mut().backlog.push_back(message),
            }
        }
    }
}

const INSTRUCTIONS: &str = "RequestTailor collection tools. list_requests and list_environments show what \
exists; get_request shows a request file (secrets only as {{names}}); send_request sends one request and \
returns the masked report; run_folder runs a folder in order. What you may send depends on the \
environment's agent access (none, read, write). Writing requests against production environments wait \
for the user's approval; a refusal or timeout means nothing was sent. Secret values are never shown.";

/// `Ok(None)` for an empty line.
fn parse_line(line: &str) -> Result<Option<Value>, RpcError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(line)
        .map(Some)
        .map_err(|e| RpcError::new(PARSE_ERROR, format!("parse error: {e}")))
}

pub fn error_response(id: &Value, e: &RpcError) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": e.code, "message": e.message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines() {
        assert_eq!(parse_line("  ").unwrap(), None);
        assert_eq!(parse_line("{\"a\":1}").unwrap(), Some(json!({"a": 1})));
        assert_eq!(parse_line("{nope").unwrap_err().code, PARSE_ERROR);
    }

    #[test]
    fn versions_are_newest_first() {
        assert_eq!(PROTOCOL_VERSIONS[0], "2025-06-18");
        assert!(PROTOCOL_VERSIONS.contains(&"2025-03-26"));
    }
}
