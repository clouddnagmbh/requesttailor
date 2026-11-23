// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The demo server's tools (see the table in the module above).
//!
//! Wrong arguments are tool errors (`isError` with a message saying what to change), as
//! MCP 2025-11-25 recommends, so a model can correct itself; an unknown tool is a JSON-RPC
//! error.

use serde_json::{Map, Value, json};

use super::{INVALID_PARAMS, RpcError, Session, Todo, fnv, png, splitmix, todo_json};

/// Tools per `tools/list` page.
pub(super) const PAGE: usize = 4;

/// Steps `countdown` accepts at most.
const MAX_COUNTDOWN: u64 = 10;

pub(super) fn list() -> Vec<Value> {
    let object = |properties: Value, required: &[&str]| json!({ "type": "object", "properties": properties, "required": required });
    let todo = json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer" },
            "title": { "type": "string" },
            "done": { "type": "boolean" }
        },
        "required": ["id", "title", "done"]
    });
    vec![
        json!({
            "name": "echo",
            "title": "Echo",
            "description": "Returns the text it gets. The simplest possible call.",
            "inputSchema": object(json!({ "text": { "type": "string", "description": "What to echo back" } }), &["text"]),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "add",
            "title": "Add two numbers",
            "description": "Adds a and b. Answers with structured output that matches its outputSchema.",
            "inputSchema": object(json!({
                "a": { "type": "number", "description": "First number" },
                "b": { "type": "number", "description": "Second number" }
            }), &["a", "b"]),
            "outputSchema": object(json!({ "sum": { "type": "number" } }), &["sum"]),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "fake_weather",
            "title": "Fake weather",
            "description": "Made-up weather for any city. Deterministic: the same city always gets the same weather.",
            "inputSchema": object(json!({
                "city": { "type": "string", "description": "Any city name, e.g. Vienna" },
                "units": {
                    "type": "string",
                    "enum": ["metric", "imperial"],
                    "default": "metric",
                    "description": "metric: °C and km/h; imperial: °F and mph"
                }
            }), &["city"]),
            "outputSchema": object(json!({
                "city": { "type": "string" },
                "temperature": { "type": "number" },
                "units": { "type": "string" },
                "conditions": { "type": "string" },
                "humidity": { "type": "integer", "description": "Percent" },
                "wind": { "type": "number" }
            }), &["city", "temperature", "units", "conditions", "humidity", "wind"]),
            "annotations": { "readOnlyHint": true, "idempotentHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "todo_add",
            "title": "Add a to-do",
            "description": "Adds a to-do to this session's list (kept in memory until you disconnect).",
            "inputSchema": object(json!({ "title": { "type": "string", "description": "What to do" } }), &["title"]),
            "outputSchema": todo,
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }
        }),
        json!({
            "name": "todo_list",
            "title": "List to-dos",
            "description": "Lists this session's to-dos.",
            "inputSchema": object(json!({
                "open_only": { "type": "boolean", "description": "Only the ones not done yet", "default": false }
            }), &[]),
            "outputSchema": object(json!({
                "items": { "type": "array", "items": todo },
                "open": { "type": "integer" }
            }), &["items", "open"]),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "todo_done",
            "title": "Mark a to-do done",
            "description": "Marks the to-do with this id as done. Unknown ids are a tool error.",
            "inputSchema": object(json!({ "id": { "type": "integer", "description": "From todo_add or todo_list", "minimum": 1 } }), &["id"]),
            "outputSchema": todo,
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "uuid",
            "title": "UUID",
            "description": "Returns a UUID in version 4 format. Deterministic per session (the first one is always the same), so saved tests stay stable.",
            "inputSchema": object(json!({}), &[]),
            "outputSchema": object(json!({ "uuid": { "type": "string" } }), &["uuid"]),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "image",
            "title": "Image",
            "description": "Returns a small PNG image, to show image results.",
            "inputSchema": object(json!({
                "color": { "type": "string", "enum": ["blue", "green", "orange", "purple"], "default": "blue" }
            }), &[]),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "fail",
            "title": "Fail",
            "description": "Always fails with isError: true, to show how tool errors look.",
            "inputSchema": object(json!({ "message": { "type": "string", "description": "The error message" } }), &[]),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "countdown",
            "title": "Countdown",
            "description": "Counts down and sends a progress and a log notification per step before the result.",
            "inputSchema": object(json!({
                "from": { "type": "integer", "minimum": 1, "maximum": MAX_COUNTDOWN, "default": 3, "description": "Where to start" }
            }), &[]),
            "outputSchema": object(json!({ "steps": { "type": "integer" } }), &["steps"]),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
    ]
}

fn text(s: impl Into<String>) -> Value {
    json!({ "type": "text", "text": s.into() })
}

/// A result with structured content and its JSON as text (for clients without structured
/// output), plus an optional sentence first.
fn structured(summary: Option<String>, value: Value) -> Value {
    let mut content = Vec::new();
    if let Some(s) = summary {
        content.push(text(s));
    }
    content.push(text(value.to_string()));
    json!({ "content": content, "structuredContent": value })
}

fn tool_error(message: impl Into<String>) -> Value {
    json!({ "content": [text(message)], "isError": true })
}

/// The arguments as an object; anything else is a tool error.
fn arguments(args: &Value) -> Result<Map<String, Value>, Value> {
    match args {
        Value::Object(m) => Ok(m.clone()),
        Value::Null => Ok(Map::new()),
        _ => Err(tool_error("The arguments must be a JSON object.")),
    }
}

pub(super) fn call(
    session: &mut Session,
    name: &str,
    args: &Value,
    progress: Option<Value>,
    notifications: &mut Vec<Value>,
) -> Result<Value, RpcError> {
    let args = match arguments(args) {
        Ok(a) => a,
        Err(e) => return Ok(e),
    };
    let string = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    Ok(match name {
        "echo" => match args.get("text").and_then(Value::as_str) {
            Some(t) => json!({ "content": [text(t)] }),
            None => tool_error("echo needs text: {\"text\": \"hello\"}"),
        },
        "add" => match (
            args.get("a").and_then(Value::as_f64),
            args.get("b").and_then(Value::as_f64),
        ) {
            (Some(a), Some(b)) => {
                let sum = a + b;
                let sum = if sum.fract() == 0.0 && sum.abs() < 1e15 {
                    json!(sum as i64)
                } else {
                    json!(sum)
                };
                structured(None, json!({ "sum": sum }))
            }
            _ => tool_error("add needs two numbers: {\"a\": 2, \"b\": 3}"),
        },
        "fake_weather" => match string("city") {
            Some(city) => {
                let units = match args.get("units").and_then(Value::as_str) {
                    None | Some("metric") => "metric",
                    Some("imperial") => "imperial",
                    Some(other) => {
                        return Ok(tool_error(format!(
                            "units must be metric or imperial, not {other:?}"
                        )));
                    }
                };
                let w = weather(city, units);
                let (t, speed) = if units == "metric" {
                    ("°C", "km/h")
                } else {
                    ("°F", "mph")
                };
                let summary = format!(
                    "{}: {} {t}, {}, humidity {} %, wind {} {speed} (fake data)",
                    w["city"].as_str().unwrap_or(city),
                    w["temperature"],
                    w["conditions"].as_str().unwrap_or(""),
                    w["humidity"],
                    w["wind"],
                );
                structured(Some(summary), w)
            }
            None => tool_error("fake_weather needs a city: {\"city\": \"Vienna\"}"),
        },
        "todo_add" => match string("title") {
            Some(title) => {
                let todo = Todo {
                    id: session.next_todo,
                    title: title.to_owned(),
                    done: false,
                };
                session.next_todo += 1;
                session.todos.push(todo.clone());
                structured(
                    Some(format!("Added to-do {}: {}", todo.id, todo.title)),
                    todo_json(&todo),
                )
            }
            None => tool_error("todo_add needs a title: {\"title\": \"Water the plants\"}"),
        },
        "todo_list" => {
            let open_only = args
                .get("open_only")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let items: Vec<Value> = session
                .todos
                .iter()
                .filter(|t| !open_only || !t.done)
                .map(todo_json)
                .collect();
            let open = session.todos.iter().filter(|t| !t.done).count();
            structured(None, json!({ "items": items, "open": open }))
        }
        "todo_done" => {
            let Some(id) = args.get("id").and_then(Value::as_u64) else {
                return Ok(tool_error("todo_done needs an id: {\"id\": 1}"));
            };
            match session.todos.iter_mut().find(|t| t.id == id) {
                Some(t) => {
                    t.done = true;
                    let t = t.clone();
                    structured(Some(format!("Done: {}", t.title)), todo_json(&t))
                }
                None => tool_error(format!(
                    "There is no to-do {id}. Call todo_list to see the ids."
                )),
            }
        }
        "uuid" => {
            session.uuids += 1;
            structured(None, json!({ "uuid": uuid(session.uuids) }))
        }
        "image" => {
            let color = match args.get("color").and_then(Value::as_str) {
                None | Some("blue") => [0x3b, 0x82, 0xf6],
                Some("green") => [0x22, 0xc5, 0x5e],
                Some("orange") => [0xf9, 0x73, 0x16],
                Some("purple") => [0xa8, 0x55, 0xf7],
                Some(other) => {
                    return Ok(tool_error(format!(
                        "color must be blue, green, orange or purple, not {other:?}"
                    )));
                }
            };
            json!({ "content": [
                { "type": "image", "mimeType": "image/png", "data": png::badge(color) },
                text("A 64×64 PNG drawn by the demo server.")
            ] })
        }
        "fail" => {
            let message = string("message").unwrap_or("This tool always fails.");
            tool_error(message)
        }
        "countdown" => {
            let from = match args.get("from") {
                None | Some(Value::Null) => 3,
                Some(v) => match v.as_u64().filter(|n| (1..=MAX_COUNTDOWN).contains(n)) {
                    Some(n) => n,
                    None => {
                        return Ok(tool_error(format!(
                            "from must be a whole number from 1 to {MAX_COUNTDOWN}"
                        )));
                    }
                },
            };
            for step in 1..=from {
                let left = from - step;
                if let Some(token) = &progress {
                    notifications.push(json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/progress",
                        "params": {
                            "progressToken": token,
                            "progress": step,
                            "total": from,
                            "message": format!("step {step} of {from}")
                        }
                    }));
                }
                if let Some(log) = session.log("info", json!(format!("T minus {}", left + 1))) {
                    notifications.push(log);
                }
            }
            structured(Some("Liftoff!".into()), json!({ "steps": from }))
        }
        other => {
            return Err((
                INVALID_PARAMS,
                format!("Unknown tool: {other}. tools/list lists the tools."),
            ));
        }
    })
}

const CONDITIONS: [&str; 6] = [
    "sunny",
    "partly cloudy",
    "cloudy",
    "light rain",
    "thunderstorms",
    "fog",
];

/// The fake weather of `city`: the same for the same name, whatever its case.
pub(super) fn weather(city: &str, units: &str) -> Value {
    let city = city.trim();
    let h = fnv(&city.to_lowercase());
    let celsius = (h % 36) as i64 - 5;
    let conditions = if celsius <= 1 && (h >> 40) % 2 == 0 {
        "snow"
    } else {
        CONDITIONS[((h >> 8) % CONDITIONS.len() as u64) as usize]
    };
    let humidity = 30 + (h >> 16) % 61;
    let kmh = (h >> 24) % 41;
    let (temperature, wind) = if units == "imperial" {
        (
            (celsius as f64 * 9.0 / 5.0 + 32.0).round() as i64,
            (kmh as f64 * 0.621_371).round() as u64,
        )
    } else {
        (celsius, kmh)
    };
    // The name as given, first letter upper case: "vienna" reads as "Vienna".
    let mut chars = city.chars();
    let name: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    json!({
        "city": name,
        "temperature": temperature,
        "units": units,
        "conditions": conditions,
        "humidity": humidity,
        "wind": wind
    })
}

/// The `n`-th UUID of a session (version 4 layout, RFC 9562).
fn uuid(n: u64) -> String {
    let hi = splitmix(n.wrapping_mul(2));
    let lo = splitmix(n.wrapping_mul(2) + 1);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hi.to_be_bytes());
    bytes[8..].copy_from_slice(&lo.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
