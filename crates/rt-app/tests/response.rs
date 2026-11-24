// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests for `ResponseStore` (wave 3, strand C): the full body stays on the Rust
//! side, the UI reads it by lines, bytes or JSONPath.

use std::time::{Duration, Instant};

use base64::Engine as _;
use rt_app::{AppError, ResponseStore};
use rt_core::send::{REPORT_VERSION, ReceivedResponse, ReportBody, SendError, SendReport};
use rt_core::transport::{HttpVersion, Timing, TransportError};

fn report(body: ReportBody) -> SendReport {
    let size = match &body {
        ReportBody::Utf8(s) => s.len() as u64,
        ReportBody::Base64(b) => base64::engine::general_purpose::STANDARD
            .decode(b)
            .unwrap()
            .len() as u64,
    };
    SendReport {
        tests: Vec::new(),
        assertions: Vec::new(),
        logs: Vec::new(),
        var_changes: Vec::new(),
        flow: None,
        notes: Vec::new(),
        version: REPORT_VERSION,
        request_path: "a.yml".into(),
        environment: None,
        request: None,
        response: Some(ReceivedResponse {
            status: 200,
            version: HttpVersion::Http11,
            headers: vec![("content-type".into(), "application/json".into())],
            url: "http://127.0.0.1/".into(),
            redirects: vec![],
            body,
            size,
            truncated: false,
            timing: Timing {
                dns: None,
                connect: None,
                tls: None,
                ttfb: Duration::from_millis(1),
            },
            total: Duration::from_millis(2),
        }),
        variables: vec![],
        unresolved: vec![],
        error: None,
        checks: Vec::new(),
        rate_limit: None,
        segments: Vec::new(),
    }
}

fn text(body: &str) -> SendReport {
    report(ReportBody::Utf8(body.to_owned()))
}

fn binary(body: &[u8]) -> SendReport {
    report(ReportBody::Base64(
        base64::engine::general_purpose::STANDARD.encode(body),
    ))
}

fn all_lines(store: &ResponseStore, id: u64) -> Vec<String> {
    store.lines(id, 0, u32::MAX).unwrap().lines
}

fn preview(stored: &rt_app::StoredResponse) -> &ReportBody {
    &stored.report.response.as_ref().unwrap().body
}

#[test]
fn small_text_body_is_kept_whole_in_the_preview() {
    let mut store = ResponseStore::new();
    let body = "{\n  \"a\": 1\n}";
    let stored = store.insert(text(body));
    let id = stored.response_id.expect("id");
    assert_eq!(stored.size, body.len() as u64);
    assert_eq!(stored.line_count, 3);
    assert!(stored.is_text);
    assert_eq!(preview(&stored), &ReportBody::Utf8(body.into()));
    assert_eq!(all_lines(&store, id), ["{", "  \"a\": 1", "}"]);
}

#[test]
fn report_without_response_is_not_stored() {
    let mut store = ResponseStore::new();
    let mut r = text("x");
    r.response = None;
    r.error = Some(SendError::Transport(TransportError::Timeout));
    let stored = store.insert(r.clone());
    assert_eq!(stored.response_id, None);
    assert_eq!(stored.size, 0);
    assert_eq!(stored.line_count, 0);
    assert_eq!(stored.report, r);
}

#[test]
fn line_counting_rules() {
    let mut store = ResponseStore::new();
    let cases: &[(&str, &[&str])] = &[
        ("", &[]),
        ("\n", &[""]),
        ("a", &["a"]),
        ("a\n", &["a"]),
        ("a\nb", &["a", "b"]),
        ("a\nb\n", &["a", "b"]),
        ("a\n\nb\n\n", &["a", "", "b", ""]),
        // CRLF: the `\r` belongs to the line break; a lone `\r` stays in the line.
        ("a\r\nb\r\n", &["a", "b"]),
        ("a\rb\r\nc", &["a\rb", "c"]),
        ("a\r", &["a\r"]),
    ];
    for (body, expected) in cases {
        let stored = store.insert(text(body));
        let id = stored.response_id.unwrap();
        assert_eq!(stored.line_count, expected.len() as u64, "{body:?}");
        assert_eq!(all_lines(&store, id), *expected, "{body:?}");
    }
}

#[test]
fn line_ranges_are_clamped() {
    let mut store = ResponseStore::new();
    let id = store.insert(text("a\nb\nc\nd")).response_id.unwrap();
    let s = store.lines(id, 1, 2).unwrap();
    assert_eq!(
        (s.start, s.lines.clone()),
        (1, vec!["b".into(), "c".into()])
    );
    let s = store.lines(id, 3, 10).unwrap();
    assert_eq!(s.lines, ["d"]);
    let s = store.lines(id, 4, 10).unwrap();
    assert_eq!((s.start, s.lines.len()), (4, 0));
    let s = store.lines(id, u64::MAX, 10).unwrap();
    assert!(s.lines.is_empty());
    assert!(store.lines(id, 0, 0).unwrap().lines.is_empty());
}

#[test]
fn utf8_characters_are_never_split() {
    let mut store = ResponseStore::new();
    // A three-byte character straddling the 64 KiB preview boundary.
    let mut body = "a".repeat(ResponseStore::PREVIEW_BYTES - 1);
    body.push('€');
    body.push_str("\nü\n");
    let stored = store.insert(text(&body));
    assert!(stored.is_text);
    let ReportBody::Utf8(p) = preview(&stored) else {
        panic!("text preview expected")
    };
    assert_eq!(p.len(), ResponseStore::PREVIEW_BYTES - 1);
    assert!(body.starts_with(p.as_str()));
    let id = stored.response_id.unwrap();
    assert_eq!(stored.line_count, 2);
    assert_eq!(store.lines(id, 1, 1).unwrap().lines, ["ü"]);
    let end = ResponseStore::PREVIEW_BYTES as u64 - 1;
    assert_eq!(store.bytes(id, end, 4).unwrap(), "€\n".as_bytes());
}

#[test]
fn invalid_utf8_after_the_preview_is_replaced_in_lines() {
    let mut store = ResponseStore::new();
    // Valid UTF-8 in the first 64 KiB, invalid bytes after it: rt-core reports such a body as
    // base64, the store still treats it as text.
    let mut body = b"ok\n".to_vec();
    body.extend_from_slice("a".repeat(70_000).as_bytes());
    body.extend_from_slice(b"\nbad \xff\xfe byte\nend");
    let stored = store.insert(binary(&body));
    assert!(stored.is_text);
    let id = stored.response_id.unwrap();
    assert_eq!(stored.size, body.len() as u64);
    assert_eq!(stored.line_count, 4);
    let ReportBody::Utf8(p) = preview(&stored) else {
        panic!("text preview expected")
    };
    assert_eq!(p.len(), ResponseStore::PREVIEW_BYTES);
    assert_eq!(all_lines(&store, id)[2], "bad \u{fffd}\u{fffd} byte");
    // Bytes are always exact.
    assert_eq!(store.bytes(id, 0, 100_000).unwrap(), body);

    // Invalid UTF-8 at the start: binary.
    let stored = store.insert(binary(b"bad \xff\xfe byte\nend"));
    assert!(!stored.is_text);
}

#[test]
fn binary_bodies_have_no_lines_but_bytes() {
    let mut store = ResponseStore::new();
    let body: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
    let stored = store.insert(binary(&body));
    let id = stored.response_id.unwrap();
    assert!(!stored.is_text);
    assert_eq!(stored.size, 100_000);
    assert_eq!(stored.line_count, 0);
    // Documented: lines of a binary body are an empty slice, not an error.
    assert!(store.lines(id, 0, 10).unwrap().lines.is_empty());
    let ReportBody::Base64(p) = preview(&stored) else {
        panic!("base64 preview expected")
    };
    let p = base64::engine::general_purpose::STANDARD.decode(p).unwrap();
    assert_eq!(p, &body[..ResponseStore::PREVIEW_BYTES]);
    assert_eq!(store.bytes(id, 256, 4).unwrap(), [0, 1, 2, 3]);
    assert_eq!(store.bytes(id, 99_998, 10).unwrap(), [158, 159]);
    assert!(store.bytes(id, 200_000, 10).unwrap().is_empty());
}

#[test]
fn long_lines_are_cut_and_flagged() {
    let mut store = ResponseStore::new();
    let max = ResponseStore::MAX_LINE_CHARS;
    let body = format!(
        "short\n{}\n{}\n{}\nend",
        "x".repeat(max + 5000),
        "ä".repeat(max + 1),
        "y".repeat(max)
    );
    let id = store.insert(text(&body)).response_id.unwrap();
    let s = store.lines(id, 0, 10).unwrap();
    assert_eq!(s.lines.len(), 5);
    assert_eq!(s.cut, [1, 2]);
    assert_eq!(s.lines[1], "x".repeat(max));
    assert_eq!(s.lines[2], "ä".repeat(max));
    assert_eq!(s.lines[3].chars().count(), max);
    let s = store.lines(id, 2, 2).unwrap();
    assert_eq!((s.start, s.cut.clone()), (2, vec![0]));
}

#[test]
fn a_single_huge_line_is_cut_without_decoding_all_of_it() {
    let mut store = ResponseStore::new();
    let body = "z".repeat(30 * 1024 * 1024);
    let id = store.insert(text(&body)).response_id.unwrap();
    let started = Instant::now();
    for _ in 0..100 {
        let s = store.lines(id, 0, 1).unwrap();
        assert_eq!(s.cut, [0]);
        assert_eq!(s.lines[0].len(), ResponseStore::MAX_LINE_CHARS);
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn bytes_are_clamped_and_unknown_ids_fail() {
    let mut store = ResponseStore::new();
    let id = store.insert(text("0123456789")).response_id.unwrap();
    assert_eq!(store.bytes(id, 2, 3).unwrap(), b"234");
    assert_eq!(store.bytes(id, 8, 100).unwrap(), b"89");
    assert!(store.bytes(id, 10, 1).unwrap().is_empty());
    assert!(store.bytes(id, u64::MAX, u32::MAX).unwrap().is_empty());
    assert_eq!(store.bytes(99, 0, 1), Err(AppError::ResponseNotFound(99)));
    assert_eq!(store.lines(99, 0, 1), Err(AppError::ResponseNotFound(99)));
    assert_eq!(
        store.filter_json(99, "$"),
        Err(AppError::ResponseNotFound(99))
    );
}

#[test]
fn release_frees_and_ids_are_not_reused() {
    let mut store = ResponseStore::new();
    let a = store.insert(text("a")).response_id.unwrap();
    store.release(a);
    assert_eq!(store.lines(a, 0, 1), Err(AppError::ResponseNotFound(a)));
    assert_eq!(store.bytes(a, 0, 1), Err(AppError::ResponseNotFound(a)));
    // Releasing twice or an unknown id is a no-op.
    store.release(a);
    store.release(12345);
    let b = store.insert(text("b")).response_id.unwrap();
    assert_ne!(a, b);
    assert_eq!(all_lines(&store, b), ["b"]);
}

const JSON: &str = r#"{
  "method": "GET",
  "items": [
    {"id": 1, "name": "a", "tags": ["x"]},
    {"id": 2, "name": "b", "tags": []},
    {"id": 3, "name": "c", "tags": ["x", "y"]}
  ],
  "nested": {"id": 99}
}"#;

fn filter(expr: &str) -> Result<String, AppError> {
    let mut store = ResponseStore::new();
    let id = store.insert(text(JSON)).response_id.unwrap();
    store.filter_json(id, expr)
}

fn filtered(expr: &str) -> serde_json::Value {
    serde_json::from_str(&filter(expr).unwrap()).unwrap()
}

#[test]
fn jsonpath_selects_and_returns_pretty_json() {
    assert_eq!(filter("$.method").unwrap(), "[\n  \"GET\"\n]");
    assert_eq!(filtered("$.items[*].id"), serde_json::json!([1, 2, 3]));
    assert_eq!(filtered("$..id"), serde_json::json!([1, 2, 3, 99]));
    assert_eq!(filtered("$.items[-1].name"), serde_json::json!(["c"]));
    assert_eq!(filtered("$.items[0:2].name"), serde_json::json!(["a", "b"]));
    assert_eq!(
        filtered("$.items[?@.id > 1].name"),
        serde_json::json!(["b", "c"])
    );
    assert_eq!(
        filtered("$.items[?length(@.tags) == 2].id"),
        serde_json::json!([3])
    );
    assert_eq!(filtered("$['nested']['id']"), serde_json::json!([99]));
    assert_eq!(filtered("$.missing"), serde_json::json!([]));
    assert_eq!(filtered("$").as_array().unwrap().len(), 1);
}

#[test]
fn jsonpath_errors_are_clear() {
    for bad in [
        "",
        "method",
        "$.",
        "$[",
        "$.items[?@.id >]",
        "$..[",
        "$.a b",
    ] {
        let Err(AppError::Filter(message)) = filter(bad) else {
            panic!("{bad:?} should be rejected")
        };
        assert!(
            message.starts_with("invalid JSONPath expression"),
            "{bad:?}: {message}"
        );
    }
}

#[test]
fn filtering_a_non_json_body_is_a_clear_error() {
    let mut store = ResponseStore::new();
    let id = store
        .insert(text("{\"a\": 1,\n oops}"))
        .response_id
        .unwrap();
    let Err(AppError::Filter(message)) = store.filter_json(id, "$.a") else {
        panic!("should fail")
    };
    assert!(
        message.starts_with("response body is not valid JSON"),
        "{message}"
    );
    assert!(message.contains("line 2"), "{message}");

    let id = store.insert(binary(&[0xff, 0x00])).response_id.unwrap();
    let Err(AppError::Filter(message)) = store.filter_json(id, "$") else {
        panic!("should fail")
    };
    assert!(
        message.starts_with("response body is not valid JSON"),
        "{message}"
    );
}

#[test]
fn filtering_a_too_large_body_is_refused_before_parsing() {
    let mut store = ResponseStore::new();
    let body = format!("[{}0]", "0,".repeat(20 * 1024 * 1024));
    let id = store.insert(text(&body)).response_id.unwrap();
    let started = Instant::now();
    let Err(AppError::Filter(message)) = store.filter_json(id, "$[0]") else {
        panic!("should fail")
    };
    assert!(message.contains("too large"), "{message}");
    assert!(started.elapsed() < Duration::from_millis(100));
}

// ---- large bodies ------------------------------------------------------------------------

/// Same shape as the mock's virtual body: 63 characters plus `\n` = 64 bytes per line.
fn large_line(i: u64) -> String {
    format!("{{\"line\": {i:09}, \"data\": \"{}\"}}", "x".repeat(32))
}

fn make_large_body(size: usize) -> String {
    let lines = size / 64;
    let mut body = String::with_capacity(size);
    for i in 0..lines as u64 {
        body.push_str(&large_line(i));
        body.push('\n');
    }
    assert_eq!(body.len(), size);
    body
}

fn check_large(size: usize) {
    let body = make_large_body(size);
    let lines = (size / 64) as u64;
    let mut store = ResponseStore::new();

    let started = Instant::now();
    let stored = store.insert(text(&body));
    let insert = started.elapsed();
    drop(body);

    let id = stored.response_id.unwrap();
    assert_eq!(stored.size, size as u64);
    assert_eq!(stored.line_count, lines);
    assert!(stored.is_text);
    let ReportBody::Utf8(p) = preview(&stored) else {
        panic!("text preview")
    };
    assert_eq!(p.len(), ResponseStore::PREVIEW_BYTES);

    assert_eq!(store.lines(id, 0, 1).unwrap().lines, [large_line(0)]);
    let last = store.lines(id, lines - 1, 100).unwrap();
    assert_eq!(last.lines, [large_line(lines - 1)]);
    assert!(store.lines(id, lines, 100).unwrap().lines.is_empty());

    // Random access: a viewport's worth of lines anywhere costs the same as at the start.
    let started = Instant::now();
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..2_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let start = x % lines;
        let s = store.lines(id, start, 60).unwrap();
        assert_eq!(s.lines[0], large_line(start));
    }
    let random = started.elapsed();
    eprintln!(
        "{} MB: insert {insert:?}, 2000 random reads of 60 lines {random:?}",
        size >> 20
    );
    assert!(
        random < Duration::from_secs(5),
        "random access took {random:?}"
    );

    let tail = store.bytes(id, size as u64 - 64, 64).unwrap();
    assert_eq!(tail, format!("{}\n", large_line(lines - 1)).into_bytes());
    store.release(id);
    assert!(store.lines(id, 0, 1).is_err());
}

/// 100 MB in release builds, 20 MB in debug builds (the same code path, kept fast in CI's
/// debug test run). `large_body_100_mb` always uses 100 MB.
#[test]
fn large_body() {
    let mb = if cfg!(debug_assertions) { 20 } else { 100 };
    check_large(mb * 1024 * 1024);
}

/// The gate size. Run with `cargo test -p rt-app --test response --release -- --ignored`.
#[test]
#[ignore = "100 MB; slow in debug builds"]
fn large_body_100_mb() {
    check_large(100 * 1024 * 1024);
}
