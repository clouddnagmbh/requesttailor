// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The demo server's protocol, without a transport.

use serde_json::{Value, json};

use super::http::{HttpReply, HttpRequest, HttpServer};
use super::*;

fn request(session: &mut Session, id: u64, method: &str, params: Value) -> Exchange {
    session.handle(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
}

fn result(session: &mut Session, method: &str, params: Value) -> Value {
    let e = request(session, 1, method, params);
    let r = e.response.expect("a response");
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn call(session: &mut Session, tool: &str, args: Value) -> Value {
    result(
        session,
        "tools/call",
        json!({ "name": tool, "arguments": args }),
    )
}

/// Every item of a paginated list, and the number of pages.
fn all(session: &mut Session, method: &str, key: &str) -> (Vec<Value>, usize) {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let params = cursor.map_or(json!({}), |c| json!({ "cursor": c }));
        let r = result(session, method, params);
        pages += 1;
        items.extend(r[key].as_array().unwrap().iter().cloned());
        match r.get("nextCursor").and_then(Value::as_str) {
            Some(c) => cursor = Some(c.to_owned()),
            None => return (items, pages),
        }
    }
}

#[test]
fn initialize_negotiates_the_version_and_announces_capabilities() {
    let mut s = Session::new();
    let r = result(
        &mut s,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
    );
    assert_eq!(r["protocolVersion"], "2025-06-18");
    assert_eq!(r["serverInfo"]["name"], SERVER_NAME);
    for cap in ["tools", "resources", "prompts", "logging"] {
        assert!(r["capabilities"].get(cap).is_some(), "{cap}");
    }
    assert!(r["instructions"].as_str().unwrap().contains("fake_weather"));
    assert_eq!(s.protocol(), Some("2025-06-18"));

    let mut s = Session::new();
    let r = result(
        &mut s,
        "initialize",
        json!({ "protocolVersion": "1999-01-01" }),
    );
    assert_eq!(r["protocolVersion"], VERSIONS[0]);
}

#[test]
fn notifications_and_responses_get_no_answer() {
    let mut s = Session::new();
    assert_eq!(
        s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })),
        Exchange::default()
    );
    assert_eq!(
        s.handle(&json!({ "jsonrpc": "2.0", "id": 9, "result": {} })),
        Exchange::default()
    );
    let e = s.handle(&json!([1, 2]));
    assert_eq!(e.response.unwrap()["error"]["code"], -32600);
    let e = request(&mut s, 2, "nope/nothing", json!({}));
    assert_eq!(e.response.unwrap()["error"]["code"], -32601);
    assert_eq!(result(&mut s, "ping", json!({})), json!({}));
}

#[test]
fn every_list_is_paginated() {
    let mut s = Session::new();
    let (tools, pages) = all(&mut s, "tools/list", "tools");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "echo",
            "add",
            "fake_weather",
            "todo_add",
            "todo_list",
            "todo_done",
            "uuid",
            "image",
            "fail",
            "countdown"
        ]
    );
    assert_eq!(pages, 3);
    for t in &tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].is_string(), "{t}");
    }
    let (resources, pages) = all(&mut s, "resources/list", "resources");
    assert_eq!(resources.len(), 3);
    assert_eq!(pages, 2);
    let (prompts, pages) = all(&mut s, "prompts/list", "prompts");
    assert_eq!(prompts.len(), 2);
    assert_eq!(pages, 2);

    let e = request(&mut s, 3, "tools/list", json!({ "cursor": "x" }));
    assert_eq!(e.response.unwrap()["error"]["code"], -32602);
}

#[test]
fn tools_answer_with_text_and_structured_output() {
    let mut s = Session::new();
    assert_eq!(
        call(&mut s, "echo", json!({ "text": "hi" })),
        json!({ "content": [{ "type": "text", "text": "hi" }] })
    );
    let r = call(&mut s, "add", json!({ "a": 2, "b": 3.5 }));
    assert_eq!(r["structuredContent"], json!({ "sum": 5.5 }));
    let r = call(&mut s, "add", json!({ "a": 2, "b": 3 }));
    assert_eq!(r["structuredContent"], json!({ "sum": 5 }));
    assert_eq!(r["content"][0]["text"], "{\"sum\":5}");

    let r = call(&mut s, "fail", json!({}));
    assert_eq!(r["isError"], true);
    let r = call(&mut s, "fail", json!({ "message": "boom" }));
    assert_eq!(r["content"][0]["text"], "boom");

    let r = call(&mut s, "image", json!({ "color": "green" }));
    assert_eq!(r["content"][0]["type"], "image");
    assert_eq!(r["content"][0]["mimeType"], "image/png");

    // Wrong arguments are tool errors that say what to send; unknown tools are JSON-RPC errors.
    let r = call(&mut s, "add", json!({ "a": "two" }));
    assert_eq!(r["isError"], true);
    assert!(
        r["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"a\": 2")
    );
    let e = request(
        &mut s,
        4,
        "tools/call",
        json!({ "name": "nope", "arguments": {} }),
    );
    assert_eq!(e.response.unwrap()["error"]["code"], -32602);
}

#[test]
fn fake_weather_is_deterministic() {
    let mut s = Session::new();
    let a = call(&mut s, "fake_weather", json!({ "city": "Vienna" }));
    let b = call(
        &mut Session::new(),
        "fake_weather",
        json!({ "city": " vienna " }),
    );
    assert_eq!(a["structuredContent"], b["structuredContent"]);
    let w = &a["structuredContent"];
    assert_eq!(w["city"], "Vienna");
    assert_eq!(w["units"], "metric");
    let t = w["temperature"].as_i64().unwrap();
    assert!((-5..=30).contains(&t), "{t}");
    assert!(
        a["content"][0]["text"]
            .as_str()
            .unwrap()
            .ends_with("(fake data)")
    );
    // Pinned, so a change of the formula is a conscious one (saved tests depend on it).
    assert_eq!(
        w,
        &json!({ "city": "Vienna", "temperature": 21, "units": "metric", "conditions": "sunny", "humidity": 30, "wind": 12 })
    );
    let f = call(
        &mut s,
        "fake_weather",
        json!({ "city": "Vienna", "units": "imperial" }),
    );
    let fahrenheit = f["structuredContent"]["temperature"].as_i64().unwrap();
    assert_eq!(fahrenheit, (t as f64 * 9.0 / 5.0 + 32.0).round() as i64);
    assert_eq!(
        call(
            &mut s,
            "fake_weather",
            json!({ "city": "Vienna", "units": "kelvin" })
        )["isError"],
        true
    );
    assert_eq!(call(&mut s, "fake_weather", json!({}))["isError"], true);
}

#[test]
fn the_todo_list_belongs_to_the_session() {
    let mut s = Session::new();
    let r = call(&mut s, "todo_add", json!({ "title": "Water the plants" }));
    assert_eq!(
        r["structuredContent"],
        json!({ "id": 1, "title": "Water the plants", "done": false })
    );
    call(&mut s, "todo_add", json!({ "title": "Call Ada" }));
    let r = call(&mut s, "todo_done", json!({ "id": 1 }));
    assert_eq!(r["structuredContent"]["done"], true);
    let r = call(&mut s, "todo_list", json!({}));
    assert_eq!(r["structuredContent"]["open"], 1);
    assert_eq!(r["structuredContent"]["items"].as_array().unwrap().len(), 2);
    let r = call(&mut s, "todo_list", json!({ "open_only": true }));
    assert_eq!(
        r["structuredContent"]["items"],
        json!([{ "id": 2, "title": "Call Ada", "done": false }])
    );
    let r = call(&mut s, "todo_done", json!({ "id": 7 }));
    assert_eq!(r["isError"], true);
    let todos = result(&mut s, "resources/read", json!({ "uri": "demo://todos" }));
    assert!(
        todos["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Call Ada")
    );

    // Another session starts empty.
    let mut other = Session::new();
    let r = call(&mut other, "todo_list", json!({}));
    assert_eq!(r["structuredContent"], json!({ "items": [], "open": 0 }));
}

#[test]
fn uuids_are_version_4_and_stable_per_session() {
    let mut s = Session::new();
    let first = call(&mut s, "uuid", json!({}))["structuredContent"]["uuid"].clone();
    let second = call(&mut s, "uuid", json!({}))["structuredContent"]["uuid"].clone();
    assert_ne!(first, second);
    let again = call(&mut Session::new(), "uuid", json!({}))["structuredContent"]["uuid"].clone();
    assert_eq!(first, again);
    let u = first.as_str().unwrap();
    assert_eq!(u.len(), 36);
    assert_eq!(&u[14..15], "4");
    assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"), "{u}");
}

#[test]
fn countdown_sends_progress_and_logs_first() {
    let mut s = Session::new();
    let e = request(
        &mut s,
        5,
        "tools/call",
        json!({ "name": "countdown", "arguments": { "from": 3 }, "_meta": { "progressToken": "p1" } }),
    );
    let methods: Vec<&str> = e
        .notifications
        .iter()
        .map(|n| n["method"].as_str().unwrap())
        .collect();
    assert_eq!(
        methods,
        [
            "notifications/progress",
            "notifications/message",
            "notifications/progress",
            "notifications/message",
            "notifications/progress",
            "notifications/message"
        ]
    );
    assert_eq!(e.notifications[4]["params"]["progress"], 3);
    assert_eq!(e.notifications[4]["params"]["total"], 3);
    assert_eq!(e.notifications[0]["params"]["progressToken"], "p1");
    assert_eq!(e.notifications[1]["params"]["data"], "T minus 3");
    assert_eq!(
        e.response.unwrap()["result"]["structuredContent"],
        json!({ "steps": 3 })
    );

    // No token: no progress. Level warning: no info logs.
    result(&mut s, "logging/setLevel", json!({ "level": "warning" }));
    let e = request(
        &mut s,
        6,
        "tools/call",
        json!({ "name": "countdown", "arguments": {} }),
    );
    assert!(e.notifications.is_empty());
    let e = request(&mut s, 7, "logging/setLevel", json!({ "level": "loud" }));
    assert_eq!(e.response.unwrap()["error"]["code"], -32602);
    assert_eq!(
        call(&mut s, "countdown", json!({ "from": 99 }))["isError"],
        true
    );
}

#[test]
fn resources_and_prompts() {
    let mut s = Session::new();
    let r = result(&mut s, "resources/read", json!({ "uri": "demo://readme" }));
    assert_eq!(r["contents"][0]["mimeType"], "text/markdown");
    assert!(
        r["contents"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("# RequestTailor demo server")
    );
    let r = result(
        &mut s,
        "resources/read",
        json!({ "uri": "demo://cities.json" }),
    );
    let cities: Value = serde_json::from_str(r["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(cities["cities"].as_array().unwrap().len(), 6);
    let e = request(&mut s, 8, "resources/read", json!({ "uri": "demo://nope" }));
    assert_eq!(e.response.unwrap()["error"]["code"], -32002);

    let r = result(
        &mut s,
        "prompts/get",
        json!({ "name": "summarize", "arguments": { "text": "Long text.", "style": "sentence" } }),
    );
    assert_eq!(r["messages"][0]["role"], "user");
    assert!(
        r["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("in one sentence:\n\nLong text.")
    );
    let e = request(
        &mut s,
        9,
        "prompts/get",
        json!({ "name": "summarize", "arguments": {} }),
    );
    assert_eq!(e.response.unwrap()["error"]["code"], -32602);
    let r = result(
        &mut s,
        "prompts/get",
        json!({ "name": "weather_report", "arguments": { "city": "Oslo" } }),
    );
    assert_eq!(r["messages"][1]["content"]["type"], "resource");
}

// ---- Streamable HTTP, without sockets ----

fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

fn post(server: &HttpServer, headers: &[(&str, &str)], body: Value) -> HttpReply {
    let mut all = h(&[
        ("Host", "127.0.0.1:48400"),
        ("Accept", "application/json, text/event-stream"),
    ]);
    all.extend(h(headers));
    server.handle(&HttpRequest {
        method: "POST",
        path: "/mcp",
        headers: &all,
        body: body.to_string().as_bytes(),
    })
}

fn json_body(r: &HttpReply) -> Value {
    serde_json::from_slice(&r.body).unwrap()
}

#[test]
fn http_sessions() {
    let server = HttpServer::new();
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-11-25" } });
    let r = post(&server, &[], init.clone());
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("application/json"));
    let a = r.header("mcp-session-id").unwrap().to_owned();
    let b = post(&server, &[], init)
        .header("mcp-session-id")
        .unwrap()
        .to_owned();
    assert_ne!(a, b);
    assert_eq!(server.session_count(), 2);

    let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert_eq!(post(&server, &[("Mcp-Session-Id", &a)], note).status, 202);

    let add = |title: &str| json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "todo_add", "arguments": { "title": title } } });
    let r = post(&server, &[("Mcp-Session-Id", &a)], add("in a"));
    assert_eq!(json_body(&r)["result"]["structuredContent"]["id"], 1);
    let r = post(&server, &[("Mcp-Session-Id", &a)], add("in a again"));
    assert_eq!(json_body(&r)["result"]["structuredContent"]["id"], 2);
    // Session b has its own list.
    let r = post(&server, &[("Mcp-Session-Id", &b)], add("in b"));
    assert_eq!(json_body(&r)["result"]["structuredContent"]["id"], 1);

    let list = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" });
    assert_eq!(post(&server, &[], list.clone()).status, 400);
    assert_eq!(
        post(&server, &[("Mcp-Session-Id", "nope")], list.clone()).status,
        404
    );
    let r = post(
        &server,
        &[
            ("Mcp-Session-Id", &a),
            ("MCP-Protocol-Version", "1999-01-01"),
        ],
        list,
    );
    assert_eq!(r.status, 400);

    let delete = |id: &str| {
        let headers = h(&[("Host", "localhost:48400"), ("Mcp-Session-Id", id)]);
        server
            .handle(&HttpRequest {
                method: "DELETE",
                path: "/mcp",
                headers: &headers,
                body: b"",
            })
            .status
    };
    assert_eq!(delete(&a), 200);
    assert_eq!(delete(&a), 404);
    assert_eq!(server.session_count(), 1);
}

#[test]
fn http_streams_notifications_before_the_response() {
    let server = HttpServer::new();
    let r = post(
        &server,
        &[],
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
    );
    let id = r.header("mcp-session-id").unwrap().to_owned();
    let countdown = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "countdown", "arguments": { "from": 2 }, "_meta": { "progressToken": 7 } } });
    let r = post(&server, &[("Mcp-Session-Id", &id)], countdown.clone());
    assert_eq!(r.header("content-type"), Some("text/event-stream"));
    let text = String::from_utf8(r.body).unwrap();
    let events: Vec<Value> = text
        .split("\n\n")
        .filter_map(|e| e.strip_prefix("event: message\ndata: "))
        .map(|d| serde_json::from_str(d).unwrap())
        .collect();
    assert_eq!(events.len(), 5);
    assert_eq!(events[4]["id"], 2);
    // A client that takes JSON only gets the response alone.
    let mut headers = h(&[
        ("Host", "127.0.0.1"),
        ("Accept", "application/json"),
        ("Mcp-Session-Id", &id),
    ]);
    headers.push(("Content-Type".into(), "application/json".into()));
    let r = server.handle(&HttpRequest {
        method: "POST",
        path: "/mcp",
        headers: &headers,
        body: countdown.to_string().as_bytes(),
    });
    assert_eq!(r.header("content-type"), Some("application/json"));
    assert_eq!(json_body(&r)["id"], 2);
}

#[test]
fn http_refuses_foreign_hosts_and_origins() {
    let server = HttpServer::new();
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
    let try_with = |headers: &[(&str, &str)]| {
        let headers = h(headers);
        server
            .handle(&HttpRequest {
                method: "POST",
                path: "/mcp",
                headers: &headers,
                body: init.to_string().as_bytes(),
            })
            .status
    };
    assert_eq!(try_with(&[("Host", "evil.example:48400")]), 403);
    assert_eq!(try_with(&[]), 403);
    assert_eq!(
        try_with(&[
            ("Host", "127.0.0.1:48400"),
            ("Origin", "https://evil.example")
        ]),
        403
    );
    assert_eq!(
        try_with(&[("Host", "[::1]:48400"), ("Origin", "null")]),
        403
    );
    assert_eq!(
        try_with(&[
            ("Host", "localhost:48400"),
            ("Origin", "http://localhost:3000")
        ]),
        200
    );

    let headers = h(&[("Host", "127.0.0.1:1"), ("Origin", "http://127.0.0.1:3000")]);
    let r = server.handle(&HttpRequest {
        method: "OPTIONS",
        path: "/mcp",
        headers: &headers,
        body: b"",
    });
    assert_eq!(r.status, 204);
    assert_eq!(
        r.header("access-control-allow-origin"),
        Some("http://127.0.0.1:3000")
    );
    let get = |path: &str| {
        let headers = h(&[("Host", "127.0.0.1:1")]);
        server
            .handle(&HttpRequest {
                method: "GET",
                path,
                headers: &headers,
                body: b"",
            })
            .status
    };
    assert_eq!(get("/mcp"), 405);
    assert_eq!(get("/"), 200);
    assert_eq!(get("/other"), 404);
}

#[test]
fn demo_urls() {
    assert!(is_demo_url(DEMO_URL));
    assert!(is_demo_url(" rt-demo://mcp/ "));
    assert!(!is_demo_url("http://127.0.0.1/mcp"));
}
