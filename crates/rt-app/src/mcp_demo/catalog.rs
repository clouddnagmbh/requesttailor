// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The demo server's resources and prompts.

use serde_json::{Value, json};

use super::tools::weather;
use super::{INVALID_PARAMS, RpcError, Session, todo_json_list};

/// Resources per `resources/list` page.
pub(super) const RESOURCE_PAGE: usize = 2;
/// Prompts per `prompts/list` page.
pub(super) const PROMPT_PAGE: usize = 1;

/// Cities of `demo://cities.json`.
const CITIES: [&str; 6] = ["Vienna", "Berlin", "Lisbon", "Oslo", "Tokyo", "Nairobi"];

const README: &str = "\
# RequestTailor demo server

A small MCP server with fake data, built into RequestTailor. It runs on your computer and
never goes to the network.

## Tools

- `echo`: returns its text.
- `add`: adds two numbers, with structured output.
- `fake_weather`: made-up weather for any city, the same every time.
- `todo_add`, `todo_list`, `todo_done`: a to-do list that lives as long as the session.
- `uuid`: a UUID, deterministic per session.
- `image`: a small PNG.
- `fail`: always a tool error.
- `countdown`: progress and log notifications before the result.

## Resources

- `demo://readme`: this text.
- `demo://cities.json`: cities to try with `fake_weather`.
- `demo://todos`: the session's to-dos as JSON.

## Prompts

- `summarize`: asks a model to summarize a text.
- `weather_report`: a short weather report for a city, with the fake weather attached.

Run it outside the app with `rt mcp-demo` (stdio) or `rt mcp-demo --http`.
";

pub(super) fn resources() -> Vec<Value> {
    vec![
        json!({
            "uri": "demo://readme",
            "name": "README",
            "title": "About the demo server",
            "description": "What the demo server offers, in Markdown.",
            "mimeType": "text/markdown",
            "size": README.len()
        }),
        json!({
            "uri": "demo://cities.json",
            "name": "cities.json",
            "title": "Cities",
            "description": "Cities to try with fake_weather, with today's fake weather.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "demo://todos",
            "name": "todos",
            "title": "To-dos of this session",
            "description": "The to-do list of the todo_* tools, as JSON.",
            "mimeType": "application/json"
        }),
    ]
}

pub(super) fn read(session: &Session, uri: &str) -> Result<Value, RpcError> {
    let (mime, text) = match uri {
        "demo://readme" => ("text/markdown", README.to_owned()),
        "demo://cities.json" => {
            let cities: Vec<Value> = CITIES.iter().map(|c| weather(c, "metric")).collect();
            (
                "application/json",
                serde_json::to_string_pretty(&json!({ "cities": cities })).expect("JSON"),
            )
        }
        "demo://todos" => (
            "application/json",
            serde_json::to_string_pretty(&json!({ "items": todo_json_list(session) }))
                .expect("JSON"),
        ),
        // -32002: "Resource not found" in the MCP specification.
        other => return Err((-32002, format!("Resource not found: {other}"))),
    };
    Ok(json!({ "contents": [{ "uri": uri, "mimeType": mime, "text": text }] }))
}

pub(super) fn prompts() -> Vec<Value> {
    vec![
        json!({
            "name": "summarize",
            "title": "Summarize a text",
            "description": "Asks the model to summarize a text, as bullets or one sentence.",
            "arguments": [
                { "name": "text", "description": "The text to summarize", "required": true },
                { "name": "style", "description": "bullets (default) or sentence", "required": false }
            ]
        }),
        json!({
            "name": "weather_report",
            "title": "Weather report",
            "description": "A short weather report for a city, with the fake weather attached.",
            "arguments": [
                { "name": "city", "description": "Any city name", "required": true }
            ]
        }),
    ]
}

pub(super) fn get_prompt(params: &Value) -> Result<Value, RpcError> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arg = |key: &str| {
        params
            .pointer(&format!("/arguments/{key}"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let missing = |key: &str| (INVALID_PARAMS, format!("{name} needs the argument {key}"));
    let user =
        |text: String| json!({ "role": "user", "content": { "type": "text", "text": text } });
    match name {
        "summarize" => {
            let text = arg("text").ok_or_else(|| missing("text"))?;
            let how = match arg("style") {
                None | Some("bullets") => "as three short bullet points",
                Some("sentence") => "in one sentence",
                Some(other) => {
                    return Err((
                        INVALID_PARAMS,
                        format!("style must be bullets or sentence, not {other:?}"),
                    ));
                }
            };
            Ok(json!({
                "description": "Summarize a text",
                "messages": [user(format!("Summarize the following text {how}:\n\n{text}"))]
            }))
        }
        "weather_report" => {
            let city = arg("city").ok_or_else(|| missing("city"))?;
            let w = weather(city, "metric");
            Ok(json!({
                "description": format!("Weather report for {}", w["city"].as_str().unwrap_or(city)),
                "messages": [
                    user(format!("Write a friendly two-sentence weather report for {} from this data:", w["city"].as_str().unwrap_or(city))),
                    { "role": "user", "content": {
                        "type": "resource",
                        "resource": { "uri": format!("demo://weather/{}", w["city"].as_str().unwrap_or(city).to_lowercase()), "mimeType": "application/json", "text": w.to_string() }
                    } }
                ]
            }))
        }
        other => Err((INVALID_PARAMS, format!("Unknown prompt: {other}"))),
    }
}
