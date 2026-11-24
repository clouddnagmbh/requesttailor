// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests (wave 17, strand B): the engine of `rt_core::snapshot` and the
//! `matchesSnapshot` assertion of `rt_core::assert`.

use rt_core::assert::{SnapshotContext, SnapshotStore, evaluate_with};
use rt_core::model::Assertion;
use rt_core::script::ScriptResponse;
use rt_core::snapshot::{
    BODY_LIMIT, Freeze, ResponseView, SnapshotBody, SnapshotDiffKind, compare, freeze, parse,
    to_text,
};
use rt_core::vars::Scope;
use serde_json::json;

fn headers(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut h = vec![
        ("Content-Type".to_owned(), "application/json".to_owned()),
        (
            "Date".to_owned(),
            "Mon, 24 Nov 2026 10:00:00 GMT".to_owned(),
        ),
        ("Set-Cookie".to_owned(), "sid=abc".to_owned()),
        ("X-Other".to_owned(), "1".to_owned()),
        ("ETag".to_owned(), "\"v1\"".to_owned()),
    ];
    h.extend(extra.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    h
}

fn view<'a>(status: u16, headers: &'a [(String, String)], body: &'a [u8]) -> ResponseView<'a> {
    ResponseView {
        status,
        headers,
        body,
    }
}

const BODY: &str = r#"{"items":[{"id":1,"createdAt":"a"},{"id":2,"createdAt":"b"}],"total":2}"#;

#[test]
fn freezing_keeps_only_allowlisted_headers_and_masks_secrets() {
    let h = headers(&[]);
    let body = br#"{"token":"s3cr3t-value","n":1}"#;
    let snap = freeze(
        &view(200, &h, body),
        &["s3cr3t-value".into()],
        &Freeze::default(),
    );
    assert_eq!(snap.headers.len(), 1, "{:?}", snap.headers);
    assert_eq!(snap.headers["content-type"], "application/json");
    assert_eq!(
        snap.body,
        SnapshotBody::Value(json!({"token": "***", "n": 1}))
    );
    let text = to_text(&snap);
    assert!(!text.contains("s3cr3t"));
    assert!(text.ends_with("}\n"));
    // The key order is fixed.
    let pos = |k: &str| text.find(&format!("\"{k}\"")).unwrap();
    assert!(pos("version") < pos("status"));
    assert!(pos("status") < pos("headers"));
    assert!(pos("headers") < pos("body"));
    assert!(pos("body") < pos("ignore"));
    assert_eq!(parse(&text).unwrap(), snap);
    // The same input gives the same bytes.
    assert_eq!(
        text,
        to_text(&freeze(
            &view(200, &h, body),
            &["s3cr3t-value".into()],
            &Freeze::default()
        ))
    );
}

#[test]
fn etag_only_on_request() {
    let h = headers(&[]);
    let with = freeze(
        &view(200, &h, b"{}"),
        &[],
        &Freeze {
            keep_etag: true,
            ignore: vec![],
        },
    );
    assert_eq!(with.headers["etag"], "\"v1\"");
    // A snapshot that has the etag also compares it.
    let other = vec![
        ("etag".to_owned(), "\"v2\"".to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    let diffs = compare(&with, &view(200, &other, b"{}"), &[]);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "headers.etag");
    assert_eq!(diffs[0].kind, SnapshotDiffKind::Changed);
}

#[test]
fn an_equal_response_matches_and_a_different_one_lists_every_place() {
    let h = headers(&[]);
    let snap = freeze(&view(200, &h, BODY.as_bytes()), &[], &Freeze::default());
    assert!(compare(&snap, &view(200, &h, BODY.as_bytes()), &[]).is_empty());
    // Key order and white space do not matter.
    let reordered =
        br#"{ "total": 2, "items": [ {"createdAt":"a","id":1}, {"createdAt":"b","id":2} ] }"#;
    assert!(compare(&snap, &view(200, &h, reordered), &[]).is_empty());

    let other =
        br#"{"items":[{"id":1,"createdAt":"a"},{"id":"2","createdAt":"b","x":true}],"total":3}"#;
    let diffs = compare(&snap, &view(404, &h, other), &[]);
    let at = |p: &str| {
        diffs
            .iter()
            .find(|d| d.path == p)
            .unwrap_or_else(|| panic!("{p}: {diffs:?}"))
    };
    assert_eq!(at("status").kind, SnapshotDiffKind::Changed);
    assert_eq!(at("body.total").kind, SnapshotDiffKind::Changed);
    assert_eq!(at("body.total").expected, Some(json!(2)));
    assert_eq!(at("body.total").actual, Some(json!(3)));
    assert_eq!(at("body.items[1].id").kind, SnapshotDiffKind::Type);
    assert_eq!(at("body.items[1].x").kind, SnapshotDiffKind::Added);
    assert_eq!(diffs.len(), 4);

    let shorter = br#"{"items":[{"id":1,"createdAt":"a"}],"total":2}"#;
    let diffs = compare(&snap, &view(200, &h, shorter), &[]);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "body.items[1]");
    assert_eq!(diffs[0].kind, SnapshotDiffKind::Removed);
}

#[test]
fn ignore_paths_with_wildcards_and_plain_paths() {
    let h = headers(&[]);
    let mut snap = freeze(&view(200, &h, BODY.as_bytes()), &[], &Freeze::default());
    let other = br#"{"items":[{"id":1,"createdAt":"x"},{"id":2,"createdAt":"y"}],"total":2}"#;
    assert_eq!(compare(&snap, &view(200, &h, other), &[]).len(), 2);

    snap.ignore = vec!["$.items[*].createdAt".into()];
    assert!(compare(&snap, &view(200, &h, other), &[]).is_empty());
    snap.ignore = vec!["res.body.items[0].createdAt".into()];
    let diffs = compare(&snap, &view(200, &h, other), &[]);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "body.items[1].createdAt");
    // A path ignores everything below it, also things that exist on one side only.
    snap.ignore = vec!["$.items".into()];
    assert!(compare(&snap, &view(200, &h, br#"{"items":[],"total":2}"#), &[]).is_empty());
    // Status and headers are addressable.
    snap.ignore = vec!["res.status".into(), "res.headers.content-type".into()];
    let ct = vec![("content-type".to_owned(), "text/plain".to_owned())];
    assert_eq!(
        compare(&snap, &view(500, &ct, BODY.as_bytes()), &[]).len(),
        0
    );
    // A wildcard is not a prefix of a shorter path.
    snap.ignore = vec!["$.items[*].createdAt.deeper".into()];
    assert_eq!(compare(&snap, &view(200, &h, other), &[]).len(), 2);
    // Bad paths are refused, and match nothing.
    assert!(rt_core::snapshot::check_ignore("items.x").is_err());
    assert!(rt_core::snapshot::check_ignore("$.a[").is_err());
    assert!(rt_core::snapshot::check_ignore("$.a[*].b['c d']").is_ok());
    assert!(rt_core::snapshot::check_ignore("res.headers.etag").is_ok());
}

#[test]
fn the_fresh_response_is_masked_like_the_snapshot() {
    let h = headers(&[]);
    let secrets = vec!["tok-123".to_owned()];
    let snap = freeze(
        &view(200, &h, br#"{"session":"tok-123"}"#),
        &secrets,
        &Freeze::default(),
    );
    assert!(compare(&snap, &view(200, &h, br#"{"session":"tok-123"}"#), &secrets).is_empty());
    // Without the secret the same response would differ.
    assert_eq!(
        compare(&snap, &view(200, &h, br#"{"session":"tok-123"}"#), &[]).len(),
        1
    );
}

#[test]
fn non_json_bodies_are_text_up_to_the_limit_then_a_digest() {
    let h = headers(&[]);
    let text = freeze(&view(200, &h, b"hello"), &[], &Freeze::default());
    assert_eq!(text.body, SnapshotBody::Value(json!("hello")));
    let diffs = compare(&text, &view(200, &h, b"hellp"), &[]);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "body");

    let big = "a".repeat(BODY_LIMIT + 1);
    let snap = freeze(&view(200, &h, big.as_bytes()), &[], &Freeze::default());
    let SnapshotBody::Digest { digest, length } = &snap.body else {
        panic!("{:?}", snap.body)
    };
    assert_eq!(digest.len(), 64);
    assert_eq!(*length as usize, BODY_LIMIT + 1);
    let text = to_text(&snap);
    assert!(text.contains("$digest") && !text.contains("aaaa"));
    assert!(compare(&snap, &view(200, &h, big.as_bytes()), &[]).is_empty());
    let changed = format!("{}b", "a".repeat(BODY_LIMIT));
    assert_eq!(
        compare(&snap, &view(200, &h, changed.as_bytes()), &[]).len(),
        1
    );
    // Exactly at the limit stays text.
    let at_limit = "a".repeat(BODY_LIMIT);
    assert!(matches!(
        freeze(&view(200, &h, at_limit.as_bytes()), &[], &Freeze::default()).body,
        SnapshotBody::Value(_)
    ));
    // Binary bodies are a digest too.
    assert!(matches!(
        freeze(&view(200, &h, &[0xff, 0xfe, 0x00]), &[], &Freeze::default()).body,
        SnapshotBody::Digest { .. }
    ));
}

#[test]
fn a_newer_file_version_is_refused() {
    let err =
        parse(r#"{"version":2,"status":200,"headers":{},"body":null,"ignore":[]}"#).unwrap_err();
    assert!(err.contains("version 2"), "{err}");
    assert!(parse("not json").is_err());
}

struct Files(Option<String>);

impl SnapshotStore for Files {
    fn read(&self, request_path: &str, snapshot: &str) -> Option<String> {
        assert_eq!(request_path, "users/get.yml");
        assert_eq!(snapshot, "get.snap.json");
        self.0.clone()
    }
}

fn assertion() -> Assertion {
    Assertion {
        expression: "res".into(),
        operator: "matchesSnapshot".into(),
        value: "get.snap.json".into(),
        description: None,
        enabled: true,
    }
}

fn script_response(body: &str) -> ScriptResponse {
    ScriptResponse {
        status: 200,
        headers: headers(&[]),
        body: body.as_bytes().to_vec(),
        time_ms: 1,
        url: "http://x".into(),
    }
}

fn run(file: Option<String>, body: &str) -> rt_core::script::AssertionResult {
    let store = Files(file);
    let results = evaluate_with(
        &[assertion()],
        &script_response(body),
        &Scope::default(),
        Some(SnapshotContext {
            store: &store,
            request_path: "users/get.yml",
            secrets: &[],
        }),
    );
    assert_eq!(results.len(), 1);
    results.into_iter().next().unwrap()
}

#[test]
fn the_assertion_passes_fails_and_never_passes_silently() {
    let h = headers(&[]);
    let file = to_text(&freeze(
        &view(200, &h, BODY.as_bytes()),
        &[],
        &Freeze::default(),
    ));

    let ok = run(Some(file.clone()), BODY);
    assert!(ok.passed, "{ok:?}");
    assert_eq!(ok.actual.as_deref(), Some("matches"));

    let bad = run(Some(file.clone()), r#"{"items":[],"total":2}"#);
    assert!(!bad.passed);
    let error = bad.error.unwrap();
    assert!(error.contains("body.items[0]"), "{error}");

    let missing = run(None, BODY);
    assert!(!missing.passed);
    assert_eq!(
        missing.error.as_deref(),
        Some("no snapshot yet; run `rt snapshot update`")
    );

    let broken = run(Some("{".into()), BODY);
    assert!(!broken.passed);
    assert!(broken.error.unwrap().contains("cannot be read"));

    // Without a store it fails too.
    let none = evaluate_with(
        &[assertion()],
        &script_response(BODY),
        &Scope::default(),
        None,
    );
    assert!(!none[0].passed);
}

#[test]
fn ignored_paths_of_the_file_apply_in_the_assertion() {
    let h = headers(&[]);
    let mut snap = freeze(&view(200, &h, BODY.as_bytes()), &[], &Freeze::default());
    snap.ignore = vec!["$.items[*].createdAt".into()];
    let file = to_text(&snap);
    let changed = r#"{"items":[{"id":1,"createdAt":"q"},{"id":2,"createdAt":"r"}],"total":2}"#;
    assert!(run(Some(file), changed).passed);
}
