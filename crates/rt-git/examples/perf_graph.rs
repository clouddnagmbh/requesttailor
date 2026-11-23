// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `git_graph_large` (wave 15, strand C), the native part: the first page of the commit
//! graph of a repository with 100 000 commits, from opening the repository to the 2000 rows of
//! the page (`Repo::open` + `Repo::commit_graph`), release build, median of `runs`.
//!
//! `cargo run --release -p rt-git --example perf_graph -- <repository> [runs] [page]` prints
//! one JSON line; `scripts/perf/graph-large.mjs` makes the repository and adds the view's part.

use std::time::Instant;

use rt_app::git::graph::GitGraphQuery;
use rt_git::{Options, Repo};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: perf_graph <repository> [runs] [page]");
    let runs: usize = args.next().map_or(5, |n| n.parse().expect("runs"));
    let page: u32 = args.next().map_or(2000, |n| n.parse().expect("page"));

    let mut times = Vec::new();
    let mut rows = 0;
    let mut more = false;
    // One warm-up (file system cache), then the measured runs.
    for run in 0..=runs {
        let start = Instant::now();
        let repo = Repo::require(std::path::Path::new(&path), &Options::default()).expect("open");
        let graph = repo
            .commit_graph(&GitGraphQuery {
                skip: 0,
                limit: page,
                refs: None,
            })
            .expect("graph");
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        rows = graph.commits.len();
        more = graph.more;
        if run > 0 {
            times.push(ms);
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    println!(
        "{{\"median_ms\":{median:.2},\"runs_ms\":[{}],\"rows\":{rows},\"more\":{more}}}",
        times
            .iter()
            .map(|t| format!("{t:.2}"))
            .collect::<Vec<_>>()
            .join(",")
    );
}
