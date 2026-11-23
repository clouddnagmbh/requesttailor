// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Streamable HTTP (MCP 2025-03-26 and later) for the demo server, without sockets: one
//! [`HttpRequest`] in, one [`HttpReply`] out. `serve::LocalServer` puts it on a loopback port.
//!
//! - `POST /mcp` with one JSON-RPC message. `initialize` starts a session and answers with
//!   `Mcp-Session-Id`; every later request needs that header (missing: 400, unknown: 404).
//!   Notifications and responses get 202. A request answers as JSON, or as
//!   `text/event-stream` when it sends notifications first (`countdown`) and the client
//!   accepts event streams. An unsupported `MCP-Protocol-Version` header is 400.
//! - `DELETE /mcp` ends the session. `GET /mcp` is 405 (no server-initiated stream).
//! - `GET /` says what this is, for someone who opens the URL in a browser.
//! - DNS rebinding protection, as the specification asks: the `Host` must be loopback
//!   (`127.0.0.1`, `localhost`, `[::1]`), and an `Origin`, when sent, too (403 otherwise).
//!   Loopback origins get CORS headers.
//!
//! Sessions live in memory, at most [`MAX_SESSIONS`] (the least recently used goes first).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};

use super::{Session, VERSIONS, splitmix};

/// Sessions kept at once.
pub const MAX_SESSIONS: usize = 64;

/// The MCP endpoint's path.
pub const PATH: &str = "/mcp";

/// An HTTP request as the server sees it. Header names in any case.
#[derive(Debug, Clone, Copy)]
pub struct HttpRequest<'a> {
    pub method: &'a str,
    /// The request target's path (a query is ignored).
    pub path: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpReply {
    fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: serde_json::to_vec(value).expect("JSON serializes"),
        }
    }

    fn text(status: u16, text: &str) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body: text.as_bytes().to_vec(),
        }
    }

    fn rpc_error(status: u16, code: i64, message: &str) -> Self {
        Self::json(
            status,
            &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": code, "message": message } }),
        )
    }

    /// The value of header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Default)]
struct Sessions {
    open: HashMap<String, (u64, Session)>,
    /// Bumped on every use: the least recently used session has the smallest stamp.
    clock: u64,
    created: u64,
}

/// The demo server's HTTP side: sessions by id.
pub struct HttpServer {
    sessions: Mutex<Sessions>,
    seed: u64,
}

impl Default for HttpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HttpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpServer")
            .field("sessions", &self.session_count())
            .finish_non_exhaustive()
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

/// `host[:port]` names this machine's loopback interface.
fn loopback_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

/// `scheme://host[:port]` with a loopback host.
fn loopback_origin(origin: &str) -> bool {
    match origin.split_once("://") {
        Some(("http" | "https", host)) => !host.contains('/') && loopback_host(host),
        _ => false,
    }
}

impl HttpServer {
    pub fn new() -> Self {
        // Session ids only need to differ between servers and sessions; nothing here is
        // secret (loopback only, fake data).
        let now = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        Self {
            sessions: Mutex::new(Sessions::default()),
            seed: splitmix(now),
        }
    }

    /// Open sessions.
    pub fn session_count(&self) -> usize {
        self.lock().open.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Sessions> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn handle(&self, request: &HttpRequest<'_>) -> HttpReply {
        let headers = request.headers;
        if !header(headers, "host").is_some_and(loopback_host) {
            return HttpReply::text(403, "Forbidden: the demo server answers on loopback only");
        }
        let origin = header(headers, "origin");
        if origin.is_some_and(|o| !loopback_origin(o)) {
            return HttpReply::text(403, "Forbidden: origin not allowed");
        }
        let path = request.path.split('?').next().unwrap_or("");
        let mut reply = match (request.method, path) {
            ("GET", "/") => HttpReply::text(
                200,
                "RequestTailor MCP demo server. MCP clients connect with Streamable HTTP to /mcp on this address.\n",
            ),
            ("OPTIONS", _) => {
                let mut r = HttpReply::new(204);
                r.headers.push((
                    "Access-Control-Allow-Methods".into(),
                    "POST, GET, DELETE, OPTIONS".into(),
                ));
                r.headers.push((
                    "Access-Control-Allow-Headers".into(),
                    "content-type, accept, authorization, mcp-session-id, mcp-protocol-version, last-event-id".into(),
                ));
                r
            }
            (_, p) if p != PATH => HttpReply::text(404, "Not found: the MCP endpoint is /mcp"),
            ("POST", _) => self.post(headers, request.body),
            ("DELETE", _) => match header(headers, "mcp-session-id") {
                None => HttpReply::text(400, "Bad Request: Mcp-Session-Id header is required"),
                Some(id) => match self.lock().open.remove(id) {
                    Some(_) => HttpReply::new(200),
                    None => HttpReply::text(404, "Session not found"),
                },
            },
            _ => {
                let mut r = HttpReply::text(405, "Method not allowed: POST or DELETE");
                r.headers.push(("Allow".into(), "POST, DELETE".into()));
                r
            }
        };
        if let Some(o) = origin {
            reply
                .headers
                .push(("Access-Control-Allow-Origin".into(), o.to_owned()));
            reply.headers.push((
                "Access-Control-Expose-Headers".into(),
                "Mcp-Session-Id, MCP-Protocol-Version".into(),
            ));
            reply.headers.push(("Vary".into(), "Origin".into()));
        }
        reply
    }

    fn post(&self, headers: &[(String, String)], body: &[u8]) -> HttpReply {
        let message: Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(e) => return HttpReply::rpc_error(400, -32700, &format!("Parse error: {e}")),
        };
        if !message.is_object() {
            return HttpReply::rpc_error(
                400,
                -32600,
                "Invalid request: one JSON-RPC message per POST",
            );
        }
        if let Some(v) = header(headers, "mcp-protocol-version")
            && !VERSIONS.contains(&v)
        {
            return HttpReply::rpc_error(
                400,
                -32600,
                &format!(
                    "Bad Request: unsupported MCP-Protocol-Version {v}; supported: {}",
                    VERSIONS.join(", ")
                ),
            );
        }
        let streams = header(headers, "accept")
            .is_some_and(|a| a.to_ascii_lowercase().contains("text/event-stream"));
        let is_initialize = message.get("method").and_then(Value::as_str) == Some("initialize")
            && message.get("id").is_some();

        let mut sessions = self.lock();
        sessions.clock += 1;
        let stamp = sessions.clock;
        let (id, exchange) = if is_initialize {
            if sessions.open.len() >= MAX_SESSIONS
                && let Some(oldest) = sessions
                    .open
                    .iter()
                    .min_by_key(|(_, (used, _))| *used)
                    .map(|(id, _)| id.clone())
            {
                sessions.open.remove(&oldest);
            }
            sessions.created += 1;
            let id = format!(
                "{:016x}{:016x}",
                splitmix(self.seed ^ sessions.created),
                splitmix(self.seed.wrapping_add(sessions.created))
            );
            let mut session = Session::new();
            let exchange = session.handle(&message);
            sessions.open.insert(id.clone(), (stamp, session));
            (Some(id), exchange)
        } else {
            let Some(id) = header(headers, "mcp-session-id") else {
                return HttpReply::rpc_error(
                    400,
                    -32600,
                    "Bad Request: Mcp-Session-Id header is required (send initialize first)",
                );
            };
            let Some((used, session)) = sessions.open.get_mut(id) else {
                return HttpReply::rpc_error(404, -32600, "Session not found; initialize again");
            };
            *used = stamp;
            (None, session.handle(&message))
        };
        drop(sessions);

        let Some(response) = exchange.response else {
            return HttpReply::new(202);
        };
        let mut reply = if streams && !exchange.notifications.is_empty() {
            let mut body = String::new();
            for m in exchange.notifications.iter().chain([&response]) {
                body.push_str("event: message\ndata: ");
                body.push_str(&m.to_string());
                body.push_str("\n\n");
            }
            HttpReply {
                status: 200,
                headers: vec![
                    ("Content-Type".into(), "text/event-stream".into()),
                    ("Cache-Control".into(), "no-cache".into()),
                ],
                body: body.into_bytes(),
            }
        } else {
            HttpReply::json(200, &response)
        };
        if let Some(id) = id {
            reply.headers.push(("Mcp-Session-Id".into(), id));
        }
        reply
    }
}
