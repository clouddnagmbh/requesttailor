// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Performance budget `sequence_render` (wave 19, strand G), the Rust half: the Mermaid text of a
//! diagram of 500 messages (model from the recorder included) in at most 20 ms, and a diagram of
//! 2 000 messages between 8 participants within the message and text limits. The SVG layout and
//! the scroll are measured in the browser (`apps/web/e2e/sequence/perf.spec.ts`).
//! `scripts/perf/sequence.mjs` runs this in a release build with `RT_PERF_OUT=<dir>` and reads
//! `<dir>/sequence.json`.

use std::time::Duration;

use rt_app::sequence::{MAX_MESSAGES, MAX_TEXT, Recorder, SequenceOptions, SequenceSource, render};
use rt_core::guard::Caller;
use rt_core::observe::{ExchangeObserver, exchange_of};
use rt_core::send::{REPORT_VERSION, ReceivedResponse, ReportBody, SendReport, SentRequest};
use rt_core::transport::{HttpVersion, Timing};
use serde_json::json;
use web_time::Instant;

const RUNS: usize = 9;

fn report(i: usize) -> SendReport {
    let url = format!("https://h{}.example.com/items/{}?page={}", i % 8, i, i % 5);
    let mut report: SendReport = serde_json::from_value(json!({
        "version": REPORT_VERSION, "request_path": format!("items/{i}.yml"), "environment": null,
        "request": null, "response": null, "variables": [], "unresolved": [], "error": null
    }))
    .unwrap();
    report.request = Some(SentRequest {
        method: "GET".into(),
        url: url.clone(),
        headers: vec![],
        body_size: 0,
    });
    report.response = Some(ReceivedResponse {
        tls: None,
        status: if i.is_multiple_of(17) { 500 } else { 200 },
        version: HttpVersion::Http11,
        headers: vec![("Content-Type".into(), "application/json".into())],
        url,
        redirects: vec![],
        size: 20,
        body: ReportBody::Utf8("{\"id\":1,\"name\":\"item\"}".into()),
        truncated: false,
        timing: Timing {
            dns: None,
            connect: None,
            tls: None,
            ttfb: Duration::from_millis(5),
        },
        total: Duration::from_millis(11),
    });
    report
}

fn recorder(exchanges: usize) -> Recorder {
    let r = Recorder::new();
    for i in 0..exchanges {
        r.observe(&exchange_of(&report(i), &[], Caller::Cli, None));
    }
    r
}

#[test]
fn sequence_render_is_measured() {
    // 500 messages = 250 exchanges.
    let r = recorder(250);
    let options = SequenceOptions::default();
    let mut times = Vec::new();
    let mut text_len = 0;
    for _ in 0..RUNS {
        let t = Instant::now();
        let model = r.model(&SequenceSource::Session, &options).unwrap();
        let text = render(&model, &options);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(model.messages.len(), 500);
        text_len = text.mermaid.len();
    }
    times.sort_by(f64::total_cmp);
    let median = times[RUNS / 2];
    println!(
        "sequence_render: 500 messages, model and Mermaid text {median:.3} ms ({text_len} bytes)"
    );

    // 2 000 messages, 8 participants: inside every limit.
    let big = recorder(1_000);
    let model = big.model(&SequenceSource::Session, &options).unwrap();
    assert_eq!(model.messages.len(), MAX_MESSAGES);
    assert_eq!(model.participants.len(), 9, "the client and 8 servers");
    let text = render(&model, &options);
    assert!(text.mermaid.len() < MAX_TEXT);
    let t = Instant::now();
    let _ = render(&model, &options);
    let big_ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("sequence_render: 2000 messages, Mermaid text {big_ms:.3} ms");

    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json =
            json!({ "mermaid_ms": median, "mermaid_2000_ms": big_ms, "bytes_500": text_len });
        std::fs::write(
            std::path::Path::new(&dir).join("sequence.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
    // The budget is a release-build number.
    if !cfg!(debug_assertions) {
        assert!(median < 20.0, "{median} ms for 500 messages");
    }
}
