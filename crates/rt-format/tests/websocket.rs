// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance: `type: websocket` items (wave 8, strand A).
//!
//! - Every WebSocket item of the Bruno corpus reads as a request with
//!   `Protocol::WebSocket` (URL, handshake headers, message or message variants, auth,
//!   settings) and is written back byte-identically when unchanged.
//! - Edits rewrite only what changed; the edited file reads back as the request. What a
//!   WebSocket item cannot hold (method, params, body, assertions, HTTP settings) is refused.
//! - New WebSocket requests are laid out like Bruno writes them.

use std::path::{Path, PathBuf};

use rt_core::model::{
    Auth, Body, Item, KeyValue, MessageKind, MessageVariant, Param, ParamKind, Protocol,
    RequestSpec, Script, ScriptPhase, Settings, Variable, WebSocketSpec,
};
use rt_format::{COLLECTION_FILE, CollectionDoc, FileSet, read_collection};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

fn corpus(name: &str) -> FileSet {
    FileSet::from_dir(&corpus_dir().join(name)).unwrap()
}

fn files(list: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: list
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

fn text(set: &FileSet, path: &str) -> String {
    String::from_utf8(set.files[path].clone()).unwrap()
}

fn spec(doc: &CollectionDoc, path: &str) -> RequestSpec {
    doc.model().request(path).unwrap().request.spec.clone()
}

fn ws(spec: &RequestSpec) -> &WebSocketSpec {
    match &spec.protocol {
        Protocol::WebSocket(ws) => ws,
        other => panic!("not a WebSocket request: {other:?}"),
    }
}

fn ws_mut(spec: &mut RequestSpec) -> &mut WebSocketSpec {
    match &mut spec.protocol {
        Protocol::WebSocket(ws) => ws,
        other => panic!("not a WebSocket request: {other:?}"),
    }
}

fn variant(title: &str, kind: MessageKind, data: &str) -> MessageVariant {
    MessageVariant {
        title: title.into(),
        selected: false,
        kind,
        data: data.into(),
    }
}

fn request_paths(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Folder(f) => request_paths(&f.items, out),
            Item::Request(r) => out.push(r.path.clone()),
        }
    }
}

/// Paths whose bytes differ between two file sets (including added and removed files).
fn changed(a: &FileSet, b: &FileSet) -> Vec<String> {
    let mut out: Vec<String> = a
        .files
        .iter()
        .filter(|(p, bytes)| b.files.get(*p) != Some(bytes))
        .map(|(p, _)| p.clone())
        .collect();
    out.extend(
        b.files
            .keys()
            .filter(|p| !a.files.contains_key(*p))
            .cloned(),
    );
    out
}

/// The corpus collections with WebSocket items and their WebSocket request paths.
fn corpus_items() -> Vec<(FileSet, Vec<String>)> {
    let roots = [
        "websockets-multi-message-yml",
        "websockets-variable-interpolation-scopes-workspace-collections-yml",
        "ssl-client-certs-tests-collection-client-certs-enabled-yml",
        "ssl-client-certs-tests-collection-client-certs-disabled-yml",
    ];
    let mut out = Vec::new();
    for root in roots {
        let input = corpus(root);
        let doc = read_collection(&input).unwrap();
        let mut paths = Vec::new();
        request_paths(&doc.model().items, &mut paths);
        paths.retain(|p| matches!(spec(&doc, p).protocol, Protocol::WebSocket(_)));
        out.push((input, paths));
    }
    out
}

#[test]
fn corpus_websocket_items_are_requests() {
    let total: usize = corpus_items().iter().map(|(_, paths)| paths.len()).sum();
    assert_eq!(total, 10, "every WebSocket item of the corpus is a request");

    let doc = read_collection(&corpus("websockets-multi-message-yml")).unwrap();
    let multi = spec(&doc, "ws-multi-msg.yml");
    assert_eq!(multi.name, "ws-multi-msg");
    assert_eq!(multi.seq, Some(1.0));
    assert_eq!(multi.method, "WS");
    assert_eq!(multi.url, "ws://localhost:8081/ws/echo");
    assert_eq!(multi.auth, Auth::Inherit);
    assert_eq!(multi.body, Body::None);
    assert!(multi.params.is_empty());
    assert_eq!(multi.settings, Settings::default());
    assert_eq!(
        ws(&multi),
        &WebSocketSpec {
            messages: vec![
                variant(
                    "message 1",
                    MessageKind::Json,
                    "{\n  \"action\": \"subscribe\"\n}"
                ),
                variant("message 2", MessageKind::Text, "hello world"),
            ],
            timeout_ms: Some(0),
            keep_alive_ms: Some(0),
        }
    );

    // A single message is one variant without a title.
    let single = spec(&doc, "ws-single-msg.yml");
    assert_eq!(
        ws(&single).messages,
        vec![variant("", MessageKind::Json, "{\n  \"foo\": \"bar\"\n}")]
    );
    // Folded text (`>-`) reads as one line.
    let text_msgs = spec(&doc, "ws-multi-text.yml");
    assert!(
        ws(&text_msgs).messages[0]
            .data
            .ends_with(" subscribe-orders")
    );
    assert!(!ws(&text_msgs).messages[0].data.contains('\n'));
    let xml = spec(&doc, "ws-multi-xml.yml");
    assert_eq!(ws(&xml).messages[1].kind, MessageKind::Xml);

    // `auth: {type: none}`, no settings: nothing inherited, no timeouts set.
    let doc = read_collection(&corpus(
        "ssl-client-certs-tests-collection-client-certs-enabled-yml",
    ))
    .unwrap();
    let wss = spec(&doc, "wss-request.yml");
    assert_eq!(wss.url, "wss://localhost:9443/ws");
    assert_eq!(wss.auth, Auth::None);
    assert_eq!(ws(&wss).timeout_ms, None);
    assert_eq!(ws(&wss).keep_alive_ms, None);
    assert_eq!(ws(&wss).messages.len(), 1);
    assert_eq!(ws(&wss).messages[0].title, "message 1");
}

#[test]
fn unchanged_websocket_requests_change_no_byte() {
    for (input, paths) in corpus_items() {
        let mut doc = read_collection(&input).unwrap();
        let original = doc.write();
        for path in &paths {
            let s = spec(&doc, path);
            doc.set_request(path, &s)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
        }
        assert_eq!(changed(&original, &doc.write()), Vec::<String>::new());
        for path in &paths {
            assert_eq!(doc.write().files[path], input.files[path], "{path}");
        }
    }
}

/// A spec that differs from `s` in every field a WebSocket item has.
fn edited(s: &RequestSpec) -> RequestSpec {
    let mut e = s.clone();
    e.name = format!("{} (edited)", s.name);
    e.seq = Some(s.seq.unwrap_or(0.0) + 10.0);
    e.url = format!("{}/edited?x={{{{y}}}}", s.url);
    e.headers
        .insert(0, KeyValue::new("Sec-WebSocket-Protocol", "chat, v2"));
    e.headers.push(KeyValue {
        name: "X-Off".into(),
        value: "{{token}}".into(),
        enabled: false,
    });
    e.auth = match &s.auth {
        Auth::Bearer { token } => Auth::Bearer {
            token: format!("{token}2"),
        },
        _ => Auth::Bearer {
            token: "{{token}}".into(),
        },
    };
    e.vars.push(Variable::new("added", "value"));
    e.vars.push(Variable {
        name: "token".into(),
        value: None,
        enabled: true,
        secret: true,
    });
    e.scripts.push(Script {
        phase: ScriptPhase::BeforeRequest,
        code: "rt.vars.set(\"a\", 1);".into(),
    });
    let w = ws_mut(&mut e);
    if let Some(first) = w.messages.first_mut() {
        first.data = format!("{}\nmore", first.data);
        first.kind = MessageKind::Text;
    }
    w.messages.push(MessageVariant {
        title: "added".into(),
        selected: true,
        kind: MessageKind::Binary,
        data: "AAEC".into(),
    });
    w.timeout_ms = Some(1500);
    w.keep_alive_ms = match w.keep_alive_ms {
        Some(0) | None => Some(30_000),
        Some(_) => None,
    };
    e
}

#[test]
fn every_corpus_websocket_request_can_be_edited_and_restored() {
    for (input, paths) in corpus_items() {
        let base = read_collection(&input).unwrap();
        let original = base.write();
        for path in &paths {
            let mut doc = base.clone();
            let before = spec(&doc, path);
            let after = edited(&before);
            doc.set_request(path, &after)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            let written = doc.write();
            assert_eq!(changed(&original, &written), vec![path.clone()], "{path}");
            let reread = read_collection(&written).unwrap();
            assert_eq!(spec(&reread, path), normalized(&after), "{path}");
            let file = text(&written, path);
            assert!(!file.contains("\nhttp:"), "{path}:\n{file}");
            assert!(file.contains("  type: websocket\n"), "{path}:\n{file}");
            // Editing back restores the request.
            doc.set_request(path, &before)
                .unwrap_or_else(|e| panic!("{path}: back: {e}"));
            let reread = read_collection(&doc.write()).unwrap();
            assert_eq!(spec(&reread, path), before, "{path}: back");
        }
    }
}

/// What the file holds of a spec: secret variables have no value.
fn normalized(s: &RequestSpec) -> RequestSpec {
    let mut s = s.clone();
    for v in &mut s.vars {
        if v.secret {
            v.value = None;
        }
    }
    s
}

fn multi_message() -> (CollectionDoc, String) {
    let input = corpus("websockets-multi-message-yml");
    let original = text(&input, "ws-multi-msg.yml");
    (read_collection(&input).unwrap(), original)
}

#[test]
fn changing_the_url_rewrites_one_line() {
    let (mut doc, before) = multi_message();
    let mut s = spec(&doc, "ws-multi-msg.yml");
    s.url = "wss://echo.example/ws".into();
    doc.set_request("ws-multi-msg.yml", &s).unwrap();
    assert_eq!(
        text(&doc.write(), "ws-multi-msg.yml"),
        before.replace(
            "  url: ws://localhost:8081/ws/echo\n",
            "  url: wss://echo.example/ws\n"
        )
    );
}

#[test]
fn editing_one_variant_keeps_the_others() {
    let (mut doc, before) = multi_message();
    let mut s = spec(&doc, "ws-multi-msg.yml");
    ws_mut(&mut s).messages[1].data = "goodbye".into();
    doc.set_request("ws-multi-msg.yml", &s).unwrap();
    assert_eq!(
        text(&doc.write(), "ws-multi-msg.yml"),
        before.replace("data: hello world\n", "data: goodbye\n")
    );

    // Selecting a variant writes `selected: true` after the title.
    let mut s = spec(&doc, "ws-multi-msg.yml");
    ws_mut(&mut s).messages[1].selected = true;
    doc.set_request("ws-multi-msg.yml", &s).unwrap();
    let after = text(&doc.write(), "ws-multi-msg.yml");
    assert!(
        after.contains(
            "    - title: message 2\n      selected: true\n      message:\n        type: text\n        data: goodbye\n"
        ),
        "{after}"
    );
    assert_eq!(spec(&doc, "ws-multi-msg.yml"), s);

    // Changing a kind keeps the data where it is.
    let mut s = spec(&doc, "ws-multi-msg.yml");
    ws_mut(&mut s).messages[0].kind = MessageKind::Text;
    doc.set_request("ws-multi-msg.yml", &s).unwrap();
    let after2 = text(&doc.write(), "ws-multi-msg.yml");
    assert_eq!(after2, after.replacen("type: json", "type: text", 1));
}

#[test]
fn a_single_message_stays_single_and_becomes_a_list_with_a_second_one() {
    let input = corpus("websockets-multi-message-yml");
    let path = "ws-single-msg.yml";
    let before = text(&input, path);
    let mut doc = read_collection(&input).unwrap();
    let mut s = spec(&doc, path);
    ws_mut(&mut s).messages[0].data = "{\"foo\": 1}".into();
    doc.set_request(path, &s).unwrap();
    let after = text(&doc.write(), path);
    assert!(
        after.contains("  message:\n    type: json\n    data: '{\"foo\": 1}'\n"),
        "{after}"
    );

    let mut s = spec(&doc, path);
    ws_mut(&mut s)
        .messages
        .push(variant("second", MessageKind::Text, "hi"));
    doc.set_request(path, &s).unwrap();
    let after = text(&doc.write(), path);
    assert!(after.contains("  message:\n    - title: \"\"\n"), "{after}");
    assert!(
        after.contains(
            "    - title: second\n      message:\n        type: text\n        data: hi\n"
        ),
        "{after}"
    );
    assert_eq!(spec(&read_collection(&doc.write()).unwrap(), path), s);

    // Back to the one message: the single form again.
    let mut s = spec(&doc, path);
    ws_mut(&mut s).messages.truncate(1);
    ws_mut(&mut s).messages[0].data = "{\n  \"foo\": \"bar\"\n}".into();
    doc.set_request(path, &s).unwrap();
    assert_eq!(text(&doc.write(), path), before);

    // No messages: `message` goes away.
    let mut s = spec(&doc, path);
    ws_mut(&mut s).messages.clear();
    doc.set_request(path, &s).unwrap();
    let after = text(&doc.write(), path);
    assert!(!after.contains("message"), "{after}");
    assert_eq!(spec(&read_collection(&doc.write()).unwrap(), path), s);
}

#[test]
fn settings_are_edited_in_place_and_removed_when_inherited() {
    let (mut doc, before) = multi_message();
    let path = "ws-multi-msg.yml";
    let mut s = spec(&doc, path);
    ws_mut(&mut s).timeout_ms = Some(5000);
    doc.set_request(path, &s).unwrap();
    assert_eq!(
        text(&doc.write(), path),
        before.replace("  timeout: 0\n", "  timeout: 5000\n")
    );
    let mut s = spec(&doc, path);
    ws_mut(&mut s).timeout_ms = None;
    ws_mut(&mut s).keep_alive_ms = None;
    doc.set_request(path, &s).unwrap();
    let after = text(&doc.write(), path);
    assert!(!after.contains("settings"), "{after}");
    assert_eq!(spec(&doc, path), s);
}

#[test]
fn what_a_websocket_item_cannot_hold_is_refused() {
    let (doc, _) = multi_message();
    let path = "ws-multi-msg.yml";
    let base = spec(&doc, path);
    let cases: Vec<(&str, RequestSpec)> = vec![
        (
            "method",
            RequestSpec {
                method: "POST".into(),
                ..base.clone()
            },
        ),
        (
            "params",
            RequestSpec {
                params: vec![Param {
                    name: "a".into(),
                    value: "1".into(),
                    kind: ParamKind::Query,
                    enabled: true,
                }],
                ..base.clone()
            },
        ),
        (
            "body",
            RequestSpec {
                body: Body::Raw {
                    kind: rt_core::model::RawKind::Text,
                    data: "x".into(),
                },
                ..base.clone()
            },
        ),
        (
            "settings",
            RequestSpec {
                settings: Settings {
                    follow_redirects: Some(false),
                    ..Settings::default()
                },
                ..base.clone()
            },
        ),
        (
            "assertions",
            RequestSpec {
                assertions: vec![rt_core::model::Assertion {
                    expression: "res.status".into(),
                    operator: "eq".into(),
                    value: "200".into(),
                    description: None,
                    enabled: true,
                }],
                ..base.clone()
            },
        ),
        (
            "auth",
            RequestSpec {
                auth: Auth::Unsupported {
                    kind: "ntlm".into(),
                },
                ..base.clone()
            },
        ),
    ];
    for (what, s) in cases {
        let mut d = doc.clone();
        let err = d.set_request(path, &s).unwrap_err().to_string();
        assert!(err.contains(what), "{what}: {err}");
        assert_eq!(d.write().files[path], doc.write().files[path], "{what}");
    }
}

#[test]
fn new_websocket_requests_are_laid_out_like_bruno() {
    let mut doc = read_collection(&files(&[(
        COLLECTION_FILE,
        "opencollection: \"1.0.0\"\ninfo:\n  name: fresh\n",
    )]))
    .unwrap();
    let spec_in = RequestSpec {
        name: "Echo".into(),
        method: "WS".into(),
        url: "ws://localhost:8081/ws/echo".into(),
        headers: vec![KeyValue::new("Sec-WebSocket-Protocol", "chat")],
        auth: Auth::Inherit,
        protocol: Protocol::WebSocket(Box::new(WebSocketSpec {
            messages: vec![
                variant("hello", MessageKind::Json, "{\n  \"a\": 1\n}"),
                MessageVariant {
                    selected: true,
                    ..variant("bye", MessageKind::Text, "bye")
                },
            ],
            timeout_ms: Some(0),
            keep_alive_ms: Some(0),
        })),
        ..RequestSpec::default()
    };
    let path = doc.add_request("", &spec_in).unwrap();
    assert_eq!(path, "echo.yml");
    assert_eq!(
        text(&doc.write(), &path),
        "info:\n  name: Echo\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: ws://localhost:8081/ws/echo\n  headers:\n    - name: Sec-WebSocket-Protocol\n      value: chat\n  message:\n    - title: hello\n      message:\n        type: json\n        data: |-\n          {\n            \"a\": 1\n          }\n    - title: bye\n      selected: true\n      message:\n        type: text\n        data: bye\n  auth: inherit\n\nsettings:\n  timeout: 0\n  keepAliveInterval: 0\n"
    );
    let reread = read_collection(&doc.write()).unwrap();
    assert_eq!(
        spec(&reread, &path),
        RequestSpec {
            seq: Some(1.0),
            ..spec_in.clone()
        }
    );

    // The smallest one: a URL.
    let minimal = RequestSpec {
        name: "Bare".into(),
        method: "WS".into(),
        url: "wss://example.com/socket".into(),
        auth: Auth::None,
        protocol: Protocol::WebSocket(Box::default()),
        ..RequestSpec::default()
    };
    let path = doc.add_request("", &minimal).unwrap();
    assert_eq!(
        text(&doc.write(), &path),
        "info:\n  name: Bare\n  type: websocket\n  seq: 2\n\nwebsocket:\n  url: wss://example.com/socket\n"
    );
}

#[test]
fn secret_variables_are_written_without_value() {
    let (mut doc, _) = multi_message();
    let path = "ws-multi-msg.yml";
    let mut s = spec(&doc, path);
    s.vars.push(Variable {
        name: "apiKey".into(),
        value: Some("s3cr3t-value".into()),
        enabled: true,
        secret: true,
    });
    doc.set_request(path, &s).unwrap();
    let after = text(&doc.write(), path);
    assert!(!after.contains("s3cr3t-value"), "{after}");
    assert!(
        after.contains("runtime:\n  variables:\n    - secret: true\n      name: apiKey\n"),
        "{after}"
    );
    // `runtime` sits between `websocket` and `settings`.
    let ws_at = after.find("websocket:").unwrap();
    let runtime_at = after.find("runtime:").unwrap();
    let settings_at = after.find("settings:").unwrap();
    assert!(ws_at < runtime_at && runtime_at < settings_at, "{after}");
}
