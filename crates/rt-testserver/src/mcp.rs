// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A small MCP server for the MCP client's tests (wave 8, strand D): Streamable HTTP
//! ([`spawn_mcp`], and `POST /mcp` on the main test server) and stdio
//! (`rt-testserver --mcp-stdio`, [`serve_stdio`]). Test infrastructure only.
//!
//! Tools (listed two per page, so clients must follow `nextCursor`):
//!
//! | Tool | Result |
//! | --- | --- |
//! | `echo {text}` | the text (annotation `readOnlyHint`) |
//! | `add {a, b}` | `structuredContent {sum}` and its JSON as text (with `outputSchema`) |
//! | `fail {message?}` | `isError: true` with the message |
//! | `secret` | the `Authorization` header (HTTP) or `$MCP_TOKEN` (stdio), to test masking |
//! | `headers` | `structuredContent` with the `mcp-protocol-version`, `mcp-session-id` and `accept` headers the server got (HTTP; empty on stdio) |
//! | `image` | a 1×1 PNG image content |
//!
//! HTTP behaviour (spec 2025-06-18): `initialize` creates a session (`Mcp-Session-Id`);
//! later requests without it get 400, with an unknown one 404. Notifications get 202.
//! `tools/call` answers as `text/event-stream` (a log notification, then the response),
//! everything else as JSON. `DELETE` ends the session; `GET` is 405. An unknown
//! `MCP-Protocol-Version` header is 400.

use std::collections::HashSet;
use std::convert::Infallible;
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{self, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::{Body, add_cors, full};

/// Protocol versions the server speaks; the first is offered when the client asks for an
/// unknown one.
pub const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

static SESSIONS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// A tiny PNG (1×1, transparent).
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// Where a message came from: what the `secret` and `headers` tools read.
struct Origin<'a> {
    /// Request headers, lowercase names (HTTP); empty on stdio.
    headers: &'a [(String, String)],
    stdio: bool,
}

impl Origin<'_> {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "echo",
            "title": "Echo",
            "description": "Returns the text it gets.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string", "description": "What to echo" } },
                "required": ["text"]
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "add",
            "description": "Adds two numbers.",
            "inputSchema": {
                "type": "object",
                "properties": { "a": { "type": "number" }, "b": { "type": "number" } },
                "required": ["a", "b"]
            },
            "outputSchema": {
                "type": "object",
                "properties": { "sum": { "type": "number" } },
                "required": ["sum"]
            }
        }),
        json!({
            "name": "fail",
            "description": "Always fails (isError).",
            "inputSchema": {
                "type": "object",
                "properties": { "message": { "type": "string" } }
            },
            "annotations": { "destructiveHint": false }
        }),
        json!({
            "name": "secret",
            "description": "Returns the credential the server got.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "headers",
            "description": "Returns the MCP headers of the request.",
            "inputSchema": { "type": "object" }
        }),
        json!({
            "name": "image",
            "description": "Returns a tiny PNG.",
            "inputSchema": { "type": "object" }
        }),
    ]
}

const PAGE: usize = 2;

fn text(s: impl Into<String>) -> Value {
    json!({ "type": "text", "text": s.into() })
}

fn call_tool(params: &Value, origin: &Origin<'_>) -> Result<Value, (i64, String)> {
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    Ok(match name {
        "echo" => match args.get("text").and_then(Value::as_str) {
            Some(t) => json!({ "content": [text(t)] }),
            None => return Err((-32602, "echo: text is required".into())),
        },
        "add" => {
            let (Some(a), Some(b)) = (
                args.get("a").and_then(Value::as_f64),
                args.get("b").and_then(Value::as_f64),
            ) else {
                return Err((-32602, "add: a and b must be numbers".into()));
            };
            let sum = a + b;
            let sum = if sum.fract() == 0.0 && sum.abs() < 1e15 {
                json!(sum as i64)
            } else {
                json!(sum)
            };
            let structured = json!({ "sum": sum });
            json!({ "content": [text(structured.to_string())], "structuredContent": structured })
        }
        "fail" => {
            let message = args
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("it failed");
            json!({ "content": [text(format!("failed: {message}"))], "isError": true })
        }
        "secret" => {
            let value = if origin.stdio {
                std::env::var("MCP_TOKEN").unwrap_or_default()
            } else {
                origin.header("authorization").unwrap_or("").to_owned()
            };
            json!({ "content": [text(value)] })
        }
        "headers" => {
            let pick = |n: &str| origin.header(n).map_or(Value::Null, |v| json!(v));
            let structured = json!({
                "protocol_version": pick("mcp-protocol-version"),
                "session_id": pick("mcp-session-id"),
                "accept": pick("accept"),
            });
            json!({ "content": [text(structured.to_string())], "structuredContent": structured })
        }
        "image" => json!({
            "content": [{ "type": "image", "mimeType": "image/png", "data": PNG }]
        }),
        other => return Err((-32602, format!("Unknown tool: {other}"))),
    })
}

/// The answer to one JSON-RPC message; `None` for notifications and responses.
fn answer(message: &Value, origin: &Origin<'_>) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Value::as_str)?;
    let params = message.get("params").cloned().unwrap_or(json!({}));
    let result = match method {
        "initialize" => {
            let asked = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            let version = if VERSIONS.contains(&asked) {
                asked
            } else {
                VERSIONS[0]
            };
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false }, "logging": {} },
                "serverInfo": { "name": "rt-testserver", "version": "1.0.0" },
                "instructions": "Test server for RequestTailor."
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let all = tools();
            let start: usize = params
                .get("cursor")
                .and_then(Value::as_str)
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let end = (start + PAGE).min(all.len());
            let mut result = json!({ "tools": all.get(start..end).unwrap_or(&[]) });
            if end < all.len() {
                result["nextCursor"] = json!(end.to_string());
            }
            Ok(result)
        }
        "tools/call" => call_tool(&params, origin),
        other => Err((-32601, format!("Method not found: {other}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

/// Serves MCP over stdin/stdout (newline-delimited JSON-RPC) until stdin closes.
pub fn serve_stdio() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let origin = Origin {
        headers: &[],
        stdio: true,
    };
    // Something on stderr, as real servers log there.
    eprintln!("rt-testserver: MCP on stdio");
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => answer(&message, &origin),
            Err(e) => Some(json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") }
            })),
        };
        if let Some(reply) = reply {
            // A log notification first, as servers may send them at any time.
            if reply.pointer("/result/content").is_some() {
                let note = json!({ "jsonrpc": "2.0", "method": "notifications/message",
                    "params": { "level": "info", "data": "calling a tool" } });
                let _ = writeln!(stdout, "{note}");
            }
            if writeln!(stdout, "{reply}")
                .and_then(|_| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    }
}

/// Binds `addr` and serves MCP over Streamable HTTP on every path. Returns the bound address.
pub async fn spawn_mcp(addr: SocketAddr) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                let service = hyper::service::service_fn(|req: Request<Incoming>| async move {
                    let req_headers = req.headers().clone();
                    let mut response = if req.method() == Method::OPTIONS {
                        full(StatusCode::NO_CONTENT, "text/plain", "")
                    } else {
                        route(req).await
                    };
                    add_cors(&req_headers, &mut response);
                    Ok::<_, Infallible>(response)
                });
                let _ = auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    Ok(local)
}

fn rpc_error(status: StatusCode, code: i64, message: &str) -> Response<Body> {
    super::json(
        status,
        json!({ "jsonrpc": "2.0", "id": null, "error": { "code": code, "message": message } }),
    )
}

/// One HTTP request to the MCP endpoint.
pub(crate) async fn route(req: Request<Incoming>) -> Response<Body> {
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_ascii_lowercase(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let origin = Origin {
        headers: &headers,
        stdio: false,
    };
    let session = origin.header("mcp-session-id").map(str::to_owned);
    let known = |s: &str| SESSIONS.lock().unwrap().contains(s);
    match *req.method() {
        Method::POST => {}
        Method::DELETE => {
            return match session {
                Some(s) if SESSIONS.lock().unwrap().remove(&s) => {
                    full(StatusCode::OK, "text/plain", "")
                }
                Some(_) => full(StatusCode::NOT_FOUND, "text/plain", "unknown session"),
                None => full(
                    StatusCode::BAD_REQUEST,
                    "text/plain",
                    "missing Mcp-Session-Id",
                ),
            };
        }
        _ => {
            let mut r = full(StatusCode::METHOD_NOT_ALLOWED, "text/plain", "");
            r.headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("POST, DELETE"));
            return r;
        }
    }
    let Ok(body) = req.into_body().collect().await else {
        return rpc_error(StatusCode::BAD_REQUEST, -32700, "unreadable body");
    };
    let message: Value = match serde_json::from_slice(&body.to_bytes()) {
        Ok(v) => v,
        Err(e) => {
            return rpc_error(
                StatusCode::BAD_REQUEST,
                -32700,
                &format!("Parse error: {e}"),
            );
        }
    };
    let initialize = message.get("method").and_then(Value::as_str) == Some("initialize");
    if !initialize {
        match &session {
            None => return rpc_error(StatusCode::BAD_REQUEST, -32600, "missing Mcp-Session-Id"),
            Some(s) if !known(s) => {
                return rpc_error(StatusCode::NOT_FOUND, -32001, "session not found");
            }
            Some(_) => {}
        }
        if let Some(v) = origin.header("mcp-protocol-version")
            && !VERSIONS.contains(&v)
        {
            return rpc_error(
                StatusCode::BAD_REQUEST,
                -32600,
                &format!("unsupported MCP-Protocol-Version {v}"),
            );
        }
    }
    let Some(reply) = answer(&message, &origin) else {
        return full(StatusCode::ACCEPTED, "text/plain", "");
    };
    let mut response = if message.get("method").and_then(Value::as_str) == Some("tools/call") {
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/message",
            "params": { "level": "info", "data": "calling a tool" } });
        let events = format!(
            ": keep-alive\n\nevent: message\ndata: {note}\n\nid: 1\nevent: message\ndata: {reply}\n\n"
        );
        let mut r = Response::new(Full::new(Bytes::from(events)).boxed());
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        r
    } else {
        super::json(StatusCode::OK, reply)
    };
    if initialize {
        let id = format!("rt-session-{}", NEXT_SESSION.fetch_add(1, Ordering::SeqCst));
        SESSIONS.lock().unwrap().insert(id.clone());
        response
            .headers_mut()
            .insert("mcp-session-id", HeaderValue::from_str(&id).expect("ascii"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio() -> Origin<'static> {
        Origin {
            headers: &[],
            stdio: true,
        }
    }

    #[test]
    fn answers_and_pages() {
        let init = answer(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}),
            &stdio(),
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], VERSIONS[0]);
        let page = answer(
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            &stdio(),
        )
        .unwrap();
        assert_eq!(page["result"]["tools"].as_array().unwrap().len(), PAGE);
        assert_eq!(page["result"]["nextCursor"], "2");
        let last = answer(
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {"cursor": "4"}}),
            &stdio(),
        )
        .unwrap();
        assert!(last["result"].get("nextCursor").is_none());
        let add = answer(
            &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "add", "arguments": {"a": 2, "b": 3}}}),
            &stdio(),
        )
        .unwrap();
        assert_eq!(add["result"]["structuredContent"], json!({"sum": 5}));
        let unknown = answer(
            &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "nope"}}),
            &stdio(),
        )
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32602);
        assert!(
            answer(
                &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                &stdio()
            )
            .is_none()
        );
    }
}
