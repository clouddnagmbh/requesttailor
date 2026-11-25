// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `notebook_open` (wave 19, strand W): reading a notebook of 50 request blocks and
//! writing it back (the Rust part of opening one; the view only lists what this returns), median
//! of 9, at most 300 ms. `node scripts/perf/run.mjs --only notebook` runs it in release.

use rt_app::notebook::{parse, render};
use web_time::Instant;

const BLOCKS: usize = 50;
const RUNS: usize = 9;

fn text() -> String {
    let mut out = String::from("# Large notebook\n\n");
    for n in 0..BLOCKS {
        out.push_str(&format!(
            "Step {n} with some *prose*, `code` and a [link](https://example.test).\n\n```rt-request\nid: b{n}\nmethod: POST\nurl: {{{{base}}}}/orders/{n}\nheaders:\n  Accept: application/json\nbody: {{\"n\": {n}, \"items\": [1, 2, 3]}}\nset:\n  id{n}: $.id\nshow:\n  - response\n  - table: $.items[*]\n```\n\n"
        ));
    }
    out
}

#[test]
fn notebook_open_is_measured() {
    let text = text();
    let mut times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        let notebook = parse(&text).unwrap();
        let written = render(&notebook).unwrap();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(written, text, "the round trip stays exact at this size");
    }
    times.sort_by(f64::total_cmp);
    let median = times[RUNS / 2];
    println!("notebook_open: median {median:.2} ms over {RUNS} runs");
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({ "notebook_open": median, "worst": times[RUNS - 1] });
        std::fs::write(
            std::path::Path::new(&dir).join("notebook.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
    if !cfg!(debug_assertions) {
        assert!(median < 300.0, "{median} ms");
    }
}
