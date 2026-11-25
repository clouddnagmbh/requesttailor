// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand A: the trace context of a send.

use std::collections::HashSet;

use rt_core::trace::{self, TraceConfig, TraceContext};
use rt_core::transport::HttpRequest;

fn request(headers: &[(&str, &str)]) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: "https://example.com/".into(),
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        body: None,
        options: Default::default(),
    }
}

fn on() -> TraceConfig {
    TraceConfig { inject: true }
}

fn traceparents(r: &HttpRequest) -> Vec<&str> {
    r.headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("traceparent"))
        .map(|(_, v)| v.as_str())
        .collect()
}

#[test]
fn off_changes_nothing_and_reports_nothing() {
    let mut r = request(&[("Accept", "*/*")]);
    let before = r.clone();
    assert!(trace::inject(&mut r, &TraceConfig::default()).is_none());
    assert_eq!(r, before);
}

#[test]
fn on_sets_exactly_one_valid_header_and_reports_it() {
    let mut r = request(&[("Accept", "*/*")]);
    let reported = trace::inject(&mut r, &on()).unwrap();
    let values = traceparents(&r);
    assert_eq!(values.len(), 1);
    let value = values[0];
    assert_eq!(value.len(), 55);
    assert!(value.starts_with("00-") && value.ends_with("-01"));
    let parsed = TraceContext::parse(value).unwrap();
    assert!(reported.injected);
    assert_eq!(reported.trace_id, parsed.trace_id_hex());
    assert_eq!(reported.span_id, parsed.span_id_hex());
    assert_eq!(r.headers[0].0, "Accept");
}

#[test]
fn ids_are_random_and_never_zero() {
    let mut seen = HashSet::new();
    for _ in 0..2000 {
        let mut r = request(&[]);
        let t = trace::inject(&mut r, &on()).unwrap();
        assert_ne!(t.trace_id, "0".repeat(32));
        assert_ne!(t.span_id, "0".repeat(16));
        assert!(seen.insert(t.trace_id));
    }
}

#[test]
fn an_existing_header_stays_untouched_in_any_case() {
    let own = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00";
    for name in ["traceparent", "TraceParent", "TRACEPARENT"] {
        let mut r = request(&[(name, own)]);
        let before = r.clone();
        let t = trace::inject(&mut r, &on()).unwrap();
        assert_eq!(r, before);
        assert!(!t.injected);
        assert_eq!(t.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    }
}

#[test]
fn an_unreadable_own_header_stays_and_nothing_is_added() {
    let mut r = request(&[("traceparent", "garbage")]);
    let before = r.clone();
    assert!(trace::inject(&mut r, &on()).is_none());
    assert_eq!(r, before);
}

#[test]
fn tracestate_and_vendor_headers_are_never_touched_or_added() {
    let mut r = request(&[("tracestate", "vendor=1"), ("Baggage", "k=v")]);
    trace::inject(&mut r, &on()).unwrap();
    assert_eq!(r.headers.len(), 3);
    assert!(
        r.headers
            .contains(&("tracestate".into(), "vendor=1".into()))
    );
    assert!(r.headers.contains(&("Baggage".into(), "k=v".into())));
    let names: Vec<_> = r.headers.iter().map(|(k, _)| k.to_lowercase()).collect();
    assert!(
        names
            .iter()
            .all(|n| matches!(n.as_str(), "tracestate" | "baggage" | "traceparent"))
    );
}

#[test]
fn a_response_id_is_read_from_traceparent_or_x_trace_id() {
    let h = |k: &str, v: &str| vec![(k.to_owned(), v.to_owned())];
    let t = trace::from_response(&h(
        "Traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
    ))
    .unwrap();
    assert_eq!(t.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    assert!(!t.injected);
    let t = trace::from_response(&h("X-Trace-Id", "4BF92F3577B34DA6A3CE929D0E0E4736")).unwrap();
    assert_eq!(t.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    assert!(t.span_id.is_empty());
    assert!(trace::from_response(&h("X-Trace-Id", "not a trace id")).is_none());
    assert!(trace::from_response(&h("X-Trace-Id", &"0".repeat(32))).is_none());
    assert!(trace::from_response(&h("Content-Type", "text/plain")).is_none());
}

// ---- through `send_with` ----

use rt_core::model::{Collection, Item, RequestEntry, RequestSpec};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::send::{SendOptions, SendReport, send_with};
use rt_core::services::Services;
use rt_core::transport::native::NativeTransport;

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("{path}: not found"))
    }
}

fn collection(url: String, inject: bool, headers: &[(&str, &str)]) -> Collection {
    let mut c = Collection {
        name: "T".into(),
        items: vec![Item::Request(RequestEntry {
            path: "get.yml".into(),
            spec: RequestSpec {
                name: "get".into(),
                method: "GET".into(),
                url,
                headers: headers
                    .iter()
                    .map(|(k, v)| rt_core::model::KeyValue {
                        name: (*k).into(),
                        value: (*v).into(),
                        enabled: true,
                    })
                    .collect(),
                ..Default::default()
            },
        })],
        ..Default::default()
    };
    c.config.trace.inject = inject;
    c
}

async fn send(collection: &Collection) -> SendReport {
    let input = PrepareInput {
        environment: None,
        globals: Vec::new(),
        runtime: Vec::new(),
    };
    let report = send_with(
        collection,
        "get.yml",
        &input,
        &NoFiles,
        &NativeTransport::new().unwrap(),
        &SendOptions::default(),
        &Services::none(),
    )
    .await;
    assert!(report.error.is_none(), "{:?}", report.error);
    report
}

/// The request headers the server saw, by lower-case name.
fn seen(report: &SendReport, name: &str) -> Option<String> {
    echoed(report)["headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h[0] == name)
        .map(|h| h[1].as_str().unwrap().to_owned())
}

fn echoed(report: &SendReport) -> serde_json::Value {
    let rt_core::send::ReportBody::Utf8(body) = &report.response.as_ref().unwrap().body else {
        panic!("not text");
    };
    serde_json::from_str(body).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_with_tracing_on_carries_the_header_and_reports_the_same_id() {
    let addr = rt_testserver::spawn_local().await;
    let report = send(&collection(format!("http://{addr}/echo"), true, &[])).await;
    let trace = report.trace.clone().unwrap();
    assert!(trace.injected);
    let sent = seen(&report, "traceparent").unwrap();
    assert_eq!(
        TraceContext::parse(&sent).unwrap().trace_id_hex(),
        trace.trace_id
    );
    // The report's request shows the header the wire carried.
    let request = report.request.as_ref().unwrap();
    assert!(
        request
            .headers
            .iter()
            .any(|(n, v)| n == "traceparent" && *v == sent)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_with_tracing_off_has_no_header_and_no_trace() {
    let addr = rt_testserver::spawn_local().await;
    let report = send(&collection(format!("http://{addr}/echo"), false, &[])).await;
    assert!(report.trace.is_none());
    assert!(seen(&report, "traceparent").is_none());
    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("trace").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_with_its_own_header_keeps_it() {
    let addr = rt_testserver::spawn_local().await;
    let own = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let report = send(&collection(
        format!("http://{addr}/echo"),
        true,
        &[("traceparent", own), ("tracestate", "a=b")],
    ))
    .await;
    assert_eq!(seen(&report, "traceparent").as_deref(), Some(own));
    assert_eq!(seen(&report, "tracestate").as_deref(), Some("a=b"));
    let trace = report.trace.unwrap();
    assert!(!trace.injected);
    assert_eq!(trace.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_id_of_the_response_is_used_when_the_request_had_none() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\nx-trace-id: 4bf92f3577b34da6a3ce929d0e0e4736\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await;
        }
    });
    // The request's own header is unreadable, so the app has no id of its own: the
    // response's id is reported.
    let report = send(&collection(
        format!("http://{addr}/"),
        true,
        &[("traceparent", "garbage")],
    ))
    .await;
    let trace = report.trace.unwrap();
    assert_eq!(trace.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    assert!(!trace.injected);
    // With tracing off nothing is read from the response.
    let report = send(&collection(format!("http://{addr}/"), false, &[])).await;
    assert!(report.trace.is_none());
}
