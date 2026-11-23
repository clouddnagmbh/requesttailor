// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `git_status_large` (wave 15, strand B): times `Repo::working_copy` on a repository
//! `scripts/perf/status.mjs` made (50 000 tracked files, 100 changed).
//!
//! `cargo run --release -p rt-git --example perf_status -- <repository> [runs] [expected]`
//! prints one JSON line: the median in milliseconds over `runs` warm runs (default 9, after one
//! run that warms the file system cache), every run, and the number of changed files. With
//! `expected` it fails if the status lists another number of files, so a fast wrong answer
//! cannot pass.

use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: perf_status <repository> [runs] [expected]");
    let runs: usize = args.next().map_or(9, |s| s.parse().expect("runs"));
    let expected: Option<usize> = args.next().map(|s| s.parse().expect("expected"));

    let repo = rt_git::Repo::require(
        std::path::Path::new(&path),
        &rt_git::Options { isolated: true },
    )
    .expect("open the repository");
    // Warm-up: the first run reads the file system cold.
    let files = repo.working_copy().expect("working copy").files.len();
    if let Some(expected) = expected {
        assert_eq!(
            files, expected,
            "the working copy lists the wrong number of files"
        );
    }
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            let wc = repo.working_copy().expect("working copy");
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(wc.files.len(), files);
            ms
        })
        .collect();
    let all = times.clone();
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    println!(
        "{}",
        serde_json::json!({ "median_ms": median, "runs_ms": all, "files": files })
    );
}
