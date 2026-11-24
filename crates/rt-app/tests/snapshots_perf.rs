// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Performance budget `snapshot_compare` (wave 17): 1000 stored responses (2 KB JSON, 20
//! ignored paths each, 100 differing) are compared with their snapshots, parsing included,
//! in under 150 ms (`perf/budgets.toml`). `scripts/perf/snapshots.mjs` runs this in a release
//! build with `RT_PERF_OUT=<dir>` and reads `<dir>/snapshots.json`.

use rt_core::snapshot::{Freeze, ResponseView, compare, freeze, parse, to_text};
use web_time::Instant;

const RESPONSES: usize = 1000;
const DIFFERING: usize = 100;
const RUNS: usize = 9;

/// About 2 KB of JSON: 20 volatile fields (`v0`..`v19`) and a list of items.
fn body(i: usize, changed: bool) -> Vec<u8> {
    let mut fields: Vec<String> = (0..20)
        .map(|k| {
            format!(
                "\"v{k}\":\"volatile-{i}-{k}-{}\"",
                if changed { "x" } else { "" }
            )
        })
        .collect();
    let items: Vec<String> = (0..12)
        .map(|n| format!("{{\"id\":{n},\"name\":\"item {n} of {i}\",\"tags\":[\"a\",\"b\"]}}"))
        .collect();
    fields.push(format!("\"items\":[{}]", items.join(",")));
    // One stable difference in the changed responses, outside the ignored fields.
    fields.push(format!("\"total\":{}", if changed { 13 } else { 12 }));
    format!("{{{}}}", fields.join(",")).into_bytes()
}

fn headers() -> Vec<(String, String)> {
    vec![("Content-Type".into(), "application/json".into())]
}

/// Snapshot texts (as stored on disk) and the responses to compare with them.
fn corpus() -> Vec<(String, Vec<u8>)> {
    let ignore: Vec<String> = (0..20).map(|k| format!("$.v{k}")).collect();
    let h = headers();
    (0..RESPONSES)
        .map(|i| {
            let frozen = body(i, false);
            let view = ResponseView {
                status: 200,
                headers: &h,
                body: &frozen,
            };
            let snap = freeze(
                &view,
                &[],
                &Freeze {
                    keep_etag: false,
                    ignore: ignore.clone(),
                },
            );
            let now = body(i, i % (RESPONSES / DIFFERING) == 0);
            (to_text(&snap), now)
        })
        .collect()
}

fn run_once(corpus: &[(String, Vec<u8>)]) -> usize {
    let h = headers();
    let mut differing = 0;
    for (text, now) in corpus {
        let snap = parse(text).expect("the snapshot reads");
        let view = ResponseView {
            status: 200,
            headers: &h,
            body: now,
        };
        if !compare(&snap, &view, &[]).is_empty() {
            differing += 1;
        }
    }
    differing
}

#[test]
fn snapshot_compare_of_1000_responses_is_measured() {
    let corpus = corpus();
    assert!(corpus[0].0.len() > 1500, "responses of about 2 KB");
    let mut runs = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        let differing = run_once(&corpus);
        runs.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(differing, DIFFERING, "100 responses differ");
    }
    runs.sort_by(f64::total_cmp);
    let median = runs[RUNS / 2];
    println!("snapshot_compare: median {median:.1} ms over {RUNS} runs");
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "snapshot_compare": median, "runs": runs });
        std::fs::write(
            std::path::Path::new(&dir).join("snapshots.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
