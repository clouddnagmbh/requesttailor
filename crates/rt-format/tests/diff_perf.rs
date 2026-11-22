// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Performance budget `semantic_diff` (wave 12): the semantic diff of a collection with 1000
//! requests, 100 of them changed, takes under 200 ms (`perf/budgets.toml`).
//!
//! `scripts/perf/run.mjs` (part `diff`) runs the measuring test in a release build with
//! `RT_PERF_OUT=<dir>`; it writes `<dir>/diff.json` (`{"semantic_diff": <median ms>,
//! "runs": [...]}`), which the budget check reads. The measurement runs from the interface
//! commit on; until strand A it times the file-level interface `diff`. What the diff must find
//! is checked by the second test, `#[ignore]`d until strand A turns it on (it then runs with
//! `cargo test`).

use rt_format::FileSet;
use rt_format::diff::{ChangeKind, ChangeTarget, diff};
use web_time::Instant;

const REQUESTS: usize = 1000;
const CHANGED: usize = 100;
const RUNS: usize = 9;

fn request(i: usize, version: &str) -> String {
    format!(
        "info:\n  name: Request {i}\n  type: http\n  seq: {i}\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/{version}/items/{i}\"\n  headers:\n    - name: Accept\n      value: application/json\n    - name: X-Trace\n      value: \"{{{{trace}}}}\"\n  auth: inherit\n\nsettings:\n  encodeUrl: true\n  timeout: 0\n"
    )
}

/// The collection before and after: 1000 requests in 20 folders; every tenth request has a
/// new URL.
fn collections() -> (FileSet, FileSet) {
    let mut old = FileSet::default();
    let mut new = FileSet::default();
    let head = b"opencollection: 1.0.0\n\ninfo:\n  name: perf\n".to_vec();
    old.files.insert("opencollection.yml".into(), head.clone());
    new.files.insert("opencollection.yml".into(), head);
    for i in 0..REQUESTS {
        let path = format!("folder-{:02}/request-{i:04}.yml", i % 20);
        old.files
            .insert(path.clone(), request(i, "v1").into_bytes());
        let version = if i % (REQUESTS / CHANGED) == 0 {
            "v2"
        } else {
            "v1"
        };
        new.files.insert(path, request(i, version).into_bytes());
    }
    (old, new)
}

#[test]
fn semantic_diff_of_1000_requests_is_measured() {
    let (old, new) = collections();
    let mut runs = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        let result = diff(&old, &new).expect("the diff runs");
        runs.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(result.files.len(), CHANGED, "100 files changed");
    }
    runs.sort_by(f64::total_cmp);
    let median = runs[RUNS / 2];
    println!("semantic_diff: median {median:.1} ms over {RUNS} runs");
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "semantic_diff": median, "runs": runs });
        std::fs::write(
            std::path::Path::new(&dir).join("diff.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn semantic_diff_of_1000_requests_finds_every_change() {
    let (old, new) = collections();
    let result = diff(&old, &new).unwrap();
    assert_eq!(result.changes.len(), CHANGED);
    for change in &result.changes {
        assert!(matches!(change.target, ChangeTarget::Request { .. }));
        assert_eq!(change.change, ChangeKind::Modified);
        assert_eq!(change.fields.len(), 1, "{}: only the URL", change.file);
        assert_eq!(change.fields[0].path, vec!["url".to_string()]);
    }
    assert!(result.files.iter().all(|f| f.covered));
}
