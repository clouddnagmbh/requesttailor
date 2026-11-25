// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `trace_inject` (wave 19, strand A): `trace::inject` with and without an existing
//! header, 100 000 calls in batches of 1000; the median call in milliseconds. Run in release
//! mode by `node scripts/perf/run.mjs --only trace`.

use std::time::Instant;

use rt_core::trace::{self, TraceConfig};
use rt_core::transport::HttpRequest;

const CALLS: usize = 100_000;
const BATCH: usize = 1000;
const LIMIT_MS: f64 = 0.02;

fn request(with_header: bool) -> HttpRequest {
    let mut headers = vec![
        ("Accept".to_owned(), "application/json".to_owned()),
        ("User-Agent".to_owned(), "rt".to_owned()),
    ];
    if with_header {
        headers.push((
            "traceparent".to_owned(),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned(),
        ));
    }
    HttpRequest {
        method: "GET".into(),
        url: "https://example.com/".into(),
        headers,
        body: None,
        options: Default::default(),
    }
}

/// Median over batches of the mean call time, in milliseconds.
fn measure(with_header: bool, config: &TraceConfig) -> f64 {
    let mut means = Vec::new();
    for _ in 0..CALLS / BATCH {
        let mut requests: Vec<HttpRequest> = (0..BATCH).map(|_| request(with_header)).collect();
        let started = Instant::now();
        for r in &mut requests {
            std::hint::black_box(trace::inject(r, config));
        }
        means.push(started.elapsed().as_secs_f64() * 1000.0 / BATCH as f64);
    }
    means.sort_by(f64::total_cmp);
    means[means.len() / 2]
}

#[test]
fn trace_inject_is_measured() {
    let on = TraceConfig { inject: true };
    let fresh = measure(false, &on);
    let existing = measure(true, &on);
    let off = measure(false, &TraceConfig::default());
    println!("trace_inject: fresh {fresh:.6} ms, existing {existing:.6} ms, off {off:.6} ms");
    let worst = fresh.max(existing);
    // Generous in debug builds; the budget itself is checked by the script on release.
    if !cfg!(debug_assertions) {
        assert!(worst < LIMIT_MS, "{worst} ms");
    }
    assert!(
        off < fresh.max(0.001),
        "off must cost nothing: {off} vs {fresh}"
    );
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({
            "trace_inject": worst,
            "fresh_ms": fresh,
            "existing_ms": existing,
            "off_ms": off,
        });
        std::fs::write(
            std::path::Path::new(&dir).join("trace.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
