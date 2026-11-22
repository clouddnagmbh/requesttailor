// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The example plugin's hooks, called through the SDK's JSON dispatch (what `rt_call` runs).

use rt_plugin_example::{Example, MANIFEST};
use rt_plugin_sdk::api::{
    Block, Call, DetectReply, ImportReply, NotesReply, Permissions, PluginRequest, PrepareReply,
    TabTarget, ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::testing::{self, MockHost};
use rt_plugin_sdk::{LossEffect, LossKind, NoteLevel, info, text_file};
use serde_json::json;

fn prepare(request: PluginRequest) -> PrepareReply {
    testing::call(
        &Example,
        &Call::Prepare {
            request,
            context: testing::context("users/get.yml"),
        },
    )
    .unwrap()
}

#[test]
fn manifest_is_valid_and_asks_for_no_rights() {
    let m = rt_plugin_sdk::parse_manifest(MANIFEST).unwrap();
    assert_eq!(m.id, "example");
    assert_eq!(m.permissions, Permissions::default());
    assert!(m.contributes.prepare && m.contributes.response);
    assert_eq!(m.contributes.tabs[0].id, "summary");
    assert_eq!(m.contributes.importers[0].id, "url-list");
}

#[test]
fn prepare_sets_a_request_id_and_notes_it() {
    let _guard = MockHost::new().now(1_790_000_000_123).install();
    let reply = prepare(testing::request("GET", "https://api.example.com/users"));
    let [(name, id)] = reply.patch.set_headers.as_slice() else {
        panic!("one header expected: {:?}", reply.patch);
    };
    assert_eq!(name, "X-Request-ID");
    assert!(id.starts_with("rt-"), "{id}");
    assert_eq!(id.len(), "rt-".len() + 12 + 1 + 8, "{id}");
    assert_eq!(reply.notes, vec![info(format!("X-Request-ID: {id}"))]);
    assert!(reply.patch.url.is_none() && reply.patch.body.is_none());

    // Same time, other request: another id.
    let other = prepare(testing::request("GET", "https://api.example.com/orders"));
    assert_ne!(other.patch.set_headers, reply.patch.set_headers);
}

#[test]
fn prepare_keeps_an_existing_request_id() {
    let _guard = MockHost::new().install();
    let mut request = testing::request("POST", "https://api.example.com/users");
    request
        .headers
        .push(("x-request-id".into(), "mine-1".into()));
    let reply = prepare(request);
    assert!(reply.patch.set_headers.is_empty());
    assert_eq!(
        reply.notes,
        vec![info("X-Request-ID: mine-1 (kept from the request)")]
    );
}

#[test]
fn prepare_never_copies_a_masked_value() {
    let _guard = MockHost::new().install();
    let reply = prepare(testing::request("GET", "https://api.example.com/?key=***"));
    let id = &reply.patch.set_headers[0].1;
    assert!(!id.contains("***"));
}

#[test]
fn response_notes_size_and_type() {
    let reply: NotesReply = testing::call(
        &Example,
        &Call::Response {
            request: testing::request("GET", "https://api.example.com/users"),
            response: testing::response(200, "application/json; charset=utf-8", &"x".repeat(2048)),
            context: testing::context("users/get.yml"),
        },
    )
    .unwrap();
    assert_eq!(
        reply.notes,
        vec![info("200 · 2.0 KB · application/json; charset=utf-8")]
    );

    let mut cut = testing::response(206, "text/plain", "abc");
    cut.truncated = true;
    cut.headers.clear();
    cut.body.content_type = None;
    let reply: NotesReply = testing::call(
        &Example,
        &Call::Response {
            request: testing::request("GET", "https://api.example.com/big"),
            response: cut,
            context: testing::context("big.yml"),
        },
    )
    .unwrap();
    assert_eq!(
        reply.notes[0].text,
        "206 · more than 3 B (cut by the host) · no content type"
    );
    assert_eq!(reply.notes[0].level, NoteLevel::Info);
}

fn tab_call(action: Option<&str>, state: Option<serde_json::Value>, with_response: bool) -> Call {
    let target = TabTarget {
        request_path: "users/get.yml".into(),
        response_id: Some(1),
    };
    let spec = RequestSpec {
        name: "Get users".into(),
        method: "GET".into(),
        url: "{{base}}/users".into(),
        ..RequestSpec::default()
    };
    let response = with_response.then(|| {
        let mut r = testing::response(200, "application/json", r#"{"a": 1, "b": [1, 2]}"#);
        r.headers.push(("X-Trace".into(), "t1".into()));
        r
    });
    match action {
        None => Call::RenderTab {
            context: None,
            tab: "summary".into(),
            target,
            spec,
            response,
            state,
        },
        Some(action) => Call::Action {
            context: None,
            tab: "summary".into(),
            action: action.into(),
            target,
            spec,
            response,
            state,
            input: vec![],
        },
    }
}

#[test]
fn summary_tab_shows_key_values_headers_and_an_action() {
    let reply: ViewReply = testing::call(&Example, &tab_call(None, None, true)).unwrap();
    let blocks = &reply.view.blocks;
    assert!(matches!(&blocks[0], Block::Heading { text } if text == "GET {{base}}/users"));
    let Block::KeyValue { rows } = &blocks[1] else {
        panic!("key-value expected: {blocks:?}");
    };
    assert!(rows.contains(&("Status".to_owned(), "200".to_owned())));
    assert!(rows.contains(&("Size".to_owned(), "21 B".to_owned())));
    let Block::Table { columns, rows } = &blocks[2] else {
        panic!("table expected: {blocks:?}");
    };
    assert_eq!(columns, &["Header", "Value"]);
    assert!(rows.contains(&vec!["X-Trace".to_owned(), "t1".to_owned()]));
    let Block::Actions { actions } = blocks.last().unwrap() else {
        panic!("actions expected: {blocks:?}");
    };
    assert_eq!(actions[0].id, "body-stats");
    assert_eq!(reply.state, Some(json!({"stats": false})));
    assert!(reply.update_request.is_none());
}

#[test]
fn body_stats_action_adds_a_block_and_keeps_it_in_the_state() {
    let reply: ViewReply =
        testing::call(&Example, &tab_call(Some("body-stats"), None, true)).unwrap();
    assert_eq!(reply.state, Some(json!({"stats": true})));
    let stats = reply
        .view
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::KeyValue { rows } if rows.iter().any(|(k, _)| k == "Lines") => Some(rows),
            _ => None,
        })
        .expect("a stats block");
    assert!(stats.contains(&("JSON".to_owned(), "object with 2 keys".to_owned())));
    let Block::Actions { actions } = reply.view.blocks.last().unwrap() else {
        panic!("actions expected");
    };
    assert_eq!(actions[0].id, "hide-stats");

    // The state brings the stats back on the next render.
    let again: ViewReply =
        testing::call(&Example, &tab_call(None, reply.state.clone(), true)).unwrap();
    assert_eq!(again.view, reply.view);

    let hidden: ViewReply =
        testing::call(&Example, &tab_call(Some("hide-stats"), reply.state, true)).unwrap();
    assert_eq!(hidden.state, Some(json!({"stats": false})));
}

#[test]
fn summary_tab_without_a_response_says_so() {
    let reply: ViewReply = testing::call(&Example, &tab_call(None, None, false)).unwrap();
    assert!(reply.view.blocks.iter().any(|b| matches!(
        b,
        Block::Notice {
            level: NoteLevel::Info,
            ..
        }
    )));
}

#[test]
fn unknown_actions_and_tabs_are_errors() {
    let reply: Result<ViewReply, String> =
        testing::call(&Example, &tab_call(Some("explode"), None, true));
    assert_eq!(reply.unwrap_err(), "unknown action: explode");
}

const LIST: &str = "\
# Users API
GET https://api.example.com/users
https://api.example.com/health
post https://api.example.com/users

DELETE https://api.example.com/users/7
";

#[test]
fn detect_accepts_url_lists_only() {
    let detect = |name: &str, text: &str| -> bool {
        let reply: DetectReply = testing::call(
            &Example,
            &Call::Detect {
                importer: "url-list".into(),
                files: vec![text_file(name, text)],
            },
        )
        .unwrap();
        reply.matches
    };
    assert!(detect("users.urls", LIST));
    assert!(detect("users.txt", "https://a.example/\n"));
    assert!(!detect("notes.txt", "Buy milk\nhttps://a.example/\n"));
    assert!(!detect("empty.txt", "# nothing\n\n"));
    assert!(!detect("users.json", "https://a.example/\n"));
}

#[test]
fn import_writes_a_collection_the_host_can_read() {
    let mut text = LIST.to_owned();
    text.push_str("FETCH me\nGET https://api.example.com/users\n");
    let reply: ImportReply = testing::call(
        &Example,
        &Call::Import {
            importer: "url-list".into(),
            files: vec![text_file("users.urls", &text)],
        },
    )
    .unwrap();
    assert_eq!(reply.collection_name, "users");
    assert_eq!(reply.report.requests, 5);
    assert_eq!(reply.report.losses.len(), 1);
    let loss = &reply.report.losses[0];
    assert_eq!(loss.location, "users.urls line 7");
    assert_eq!(
        (loss.kind, loss.effect),
        (LossKind::Other, LossEffect::Dropped)
    );

    let files = rt_format::FileSet {
        files: reply
            .files
            .iter()
            .map(|(p, t)| (p.clone(), t.clone().into_bytes()))
            .collect(),
    };
    let doc = rt_format::read_collection(&files).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    let collection = doc.model();
    assert_eq!(collection.name, "users");
    let requests: Vec<(String, String, String)> = collection
        .items
        .iter()
        .map(|item| match item {
            rt_plugin_sdk::model::Item::Request(r) => (
                r.spec.name.clone(),
                r.spec.method.clone(),
                r.spec.url.clone(),
            ),
            other => panic!("only requests expected: {other:?}"),
        })
        .collect();
    assert_eq!(
        requests,
        vec![
            (
                "GET /users".into(),
                "GET".into(),
                "https://api.example.com/users".into()
            ),
            (
                "GET /health".into(),
                "GET".into(),
                "https://api.example.com/health".into()
            ),
            (
                "POST /users".into(),
                "POST".into(),
                "https://api.example.com/users".into()
            ),
            (
                "DELETE /users/7".into(),
                "DELETE".into(),
                "https://api.example.com/users/7".into()
            ),
            (
                "GET /users".into(),
                "GET".into(),
                "https://api.example.com/users".into()
            ),
        ]
    );
}

#[test]
fn import_quotes_urls_safely() {
    let reply: ImportReply = testing::call(
        &Example,
        &Call::Import {
            importer: "url-list".into(),
            files: vec![text_file(
                "odd.txt",
                "GET https://a.example/?q=\"x\"\\y&r='z'#:z\n",
            )],
        },
    )
    .unwrap();
    let files = rt_format::FileSet {
        files: reply
            .files
            .iter()
            .map(|(p, t)| (p.clone(), t.clone().into_bytes()))
            .collect(),
    };
    let doc = rt_format::read_collection(&files).unwrap();
    let rt_plugin_sdk::model::Item::Request(r) = &doc.model().items[0] else {
        panic!("a request expected");
    };
    assert_eq!(r.spec.url, "https://a.example/?q=\"x\"\\y&r='z'#:z");
}
