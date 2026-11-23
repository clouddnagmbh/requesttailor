// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The MCP demo server: something to connect to out of the box, offline, deterministic and
//! without an account. It shows what the MCP client (`crate::mcp_client`) can do: tools
//! with forms and structured output, an image, a tool error, progress and log
//! notifications, resources, prompts and pagination.
//!
//! One implementation, three ways to reach it:
//!
//! - **Built in** ([`DEMO_URL`], every shell, also the browser): `McpClient::connect` with an
//!   HTTP server whose URL is [`DEMO_URL`] talks to a [`Session`] in memory instead of the
//!   network. Saved tests against it run anywhere, `rt mcp-test` in CI included, because
//!   nothing has to be started first.
//! - **stdio** (`rt mcp-demo`, [`serve::serve_stdio`]): newline-delimited JSON-RPC.
//! - **Streamable HTTP on loopback** (`rt mcp-demo --http`, the desktop app's "Start on a
//!   local port", [`serve::LocalServer`]): for other MCP clients and to see real HTTP.
//!   The protocol part is [`http::HttpServer`], independent of sockets.
//!
//! # What it offers
//!
//! | Tool | Shows |
//! | --- | --- |
//! | `echo {text}` | the simplest call (read-only) |
//! | `add {a, b}` | structured output with an `outputSchema` |
//! | `todo_add {title}`, `todo_list`, `todo_done {id}` | state: an in-memory to-do list per session |
//! | `fake_weather {city, units?}` | a form with an enum; deterministic fake data |
//! | `uuid` | a UUID (v4 format, deterministic per session so saved tests stay stable) |
//! | `image` | image content (a PNG drawn on the fly) |
//! | `fail {message?}` | a tool error (`isError`) |
//! | `countdown {from?}` | progress (with a `progressToken`) and log notifications |
//!
//! Resources: `demo://readme` (Markdown), `demo://cities.json` (JSON) and `demo://todos`
//! (the session's to-do list as JSON). Prompts: `summarize {text, style?}` and
//! `weather_report {city}`. Every list is paginated (tools 4, resources 2, prompts 1 per
//! page), so clients have to follow `nextCursor`.
//!
//! Every session starts empty: no to-dos, the first `uuid` is always the same. Nothing is
//! written anywhere and nothing goes to the network.

use serde_json::{Value, json};

mod catalog;
pub mod http;
mod png;
#[cfg(not(target_arch = "wasm32"))]
pub mod serve;
mod tools;

/// The built-in demo server: an MCP server config with this URL (transport `http`) is
/// answered in memory, never sent.
pub const DEMO_URL: &str = "rt-demo://mcp";

/// `serverInfo.name`.
pub const SERVER_NAME: &str = "requesttailor-demo";

/// Protocol versions the demo speaks, newest first (offered when the client asks for an
/// unknown one).
pub const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Whether `url` is the built-in demo server (surrounding blanks and a trailing `/` do not
/// matter).
pub fn is_demo_url(url: &str) -> bool {
    url.trim().trim_end_matches('/') == DEMO_URL
}

/// What one incoming message produced: notifications to send first (progress, logs), then
/// the response (`None` for notifications and responses from the client).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Exchange {
    pub notifications: Vec<Value>,
    pub response: Option<Value>,
}

/// A to-do of the `todo_*` tools.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Todo {
    id: u64,
    title: String,
    done: bool,
}

/// Log levels of `logging/setLevel`, least severe first.
const LEVELS: [&str; 8] = [
    "debug",
    "info",
    "notice",
    "warning",
    "error",
    "critical",
    "alert",
    "emergency",
];

/// One client's session: its to-do list, UUID counter and log level. Sessions share
/// nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    todos: Vec<Todo>,
    next_todo: u64,
    uuids: u64,
    /// Index into [`LEVELS`]: log notifications below it are not sent.
    log_level: usize,
    protocol: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// A JSON-RPC error: code and message.
type RpcError = (i64, String);

const INVALID_PARAMS: i64 = -32602;

impl Session {
    pub fn new() -> Self {
        Self {
            todos: Vec::new(),
            next_todo: 1,
            uuids: 0,
            log_level: 1,
            protocol: None,
        }
    }

    /// The version `initialize` agreed on, once it ran.
    pub fn protocol(&self) -> Option<&str> {
        self.protocol.as_deref()
    }

    /// Handles one JSON-RPC message (not a batch: MCP since 2025-06-18 has none).
    pub fn handle(&mut self, message: &Value) -> Exchange {
        let Some(object) = message.as_object() else {
            return Exchange {
                notifications: Vec::new(),
                response: Some(error(
                    &Value::Null,
                    -32600,
                    "Invalid request: one JSON-RPC message per line or POST".into(),
                )),
            };
        };
        let id = object.get("id").cloned();
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // A response to a request of ours: we send none, so nothing to do.
            return Exchange::default();
        };
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
        let Some(id) = id else {
            // Notifications (`notifications/initialized`, `notifications/cancelled`, …).
            return Exchange::default();
        };
        let mut notifications = Vec::new();
        let result = self.request(method, &params, &mut notifications);
        Exchange {
            notifications,
            response: Some(match result {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err((code, message)) => error(&id, code, message),
            }),
        }
    }

    fn request(
        &mut self,
        method: &str,
        params: &Value,
        notifications: &mut Vec<Value>,
    ) -> Result<Value, RpcError> {
        match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(json!({})),
            "tools/list" => page(&tools::list(), "tools", params, tools::PAGE),
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or((INVALID_PARAMS, "tools/call needs a tool name".to_owned()))?;
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let token = params.pointer("/_meta/progressToken").cloned();
                tools::call(self, name, &args, token, notifications)
            }
            "resources/list" => page(
                &catalog::resources(),
                "resources",
                params,
                catalog::RESOURCE_PAGE,
            ),
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
            "resources/read" => {
                let uri = params
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or((INVALID_PARAMS, "resources/read needs a uri".to_owned()))?;
                catalog::read(self, uri)
            }
            "prompts/list" => page(&catalog::prompts(), "prompts", params, catalog::PROMPT_PAGE),
            "prompts/get" => catalog::get_prompt(params),
            "logging/setLevel" => {
                let level = params.get("level").and_then(Value::as_str).unwrap_or("");
                let index = LEVELS.iter().position(|l| *l == level).ok_or((
                    INVALID_PARAMS,
                    format!("unknown log level {level:?}; one of {}", LEVELS.join(", ")),
                ))?;
                self.log_level = index;
                Ok(json!({}))
            }
            other => Err((-32601, format!("Method not found: {other}"))),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let asked = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("");
        let version = if VERSIONS.contains(&asked) {
            asked
        } else {
            VERSIONS[0]
        };
        self.protocol = Some(version.to_owned());
        json!({
            "protocolVersion": version,
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
                "prompts": { "listChanged": false },
                "logging": {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "title": "RequestTailor demo server",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "A demo MCP server with fake data, built into RequestTailor. Try fake_weather, keep a to-do list with todo_add, todo_list and todo_done, or watch countdown send progress. Nothing leaves your computer."
        })
    }

    /// A log notification, if `level` is at or above the session's level.
    fn log(&self, level: &str, data: Value) -> Option<Value> {
        let index = LEVELS.iter().position(|l| *l == level).unwrap_or(1);
        (index >= self.log_level).then(|| {
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/message",
                "params": { "level": level, "logger": "demo", "data": data }
            })
        })
    }
}

fn todo_json(t: &Todo) -> Value {
    json!({ "id": t.id, "title": t.title, "done": t.done })
}

fn todo_json_list(session: &Session) -> Vec<Value> {
    session.todos.iter().map(todo_json).collect()
}

fn error(id: &Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// One page of `items` under `key`, from the `cursor` in `params` (an offset as text).
fn page(items: &[Value], key: &str, params: &Value, size: usize) -> Result<Value, RpcError> {
    let start = match params.get("cursor") {
        None | Some(Value::Null) => 0,
        Some(c) => c
            .as_str()
            .and_then(|c| c.parse::<usize>().ok())
            .filter(|n| *n <= items.len())
            .ok_or((INVALID_PARAMS, format!("invalid cursor {c}")))?,
    };
    let end = (start + size).min(items.len());
    let mut result = json!({ key: items[start..end] });
    if end < items.len() {
        result["nextCursor"] = json!(end.to_string());
    }
    Ok(result)
}

/// FNV-1a: the seed of the fake data, stable across platforms and versions.
fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// SplitMix64: deterministic UUIDs.
fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests;
