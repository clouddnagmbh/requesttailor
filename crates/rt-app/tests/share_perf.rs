// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `share_url_size` (wave 17): the length of the share link of every request of the
//! Bruno corpus, 95th percentile, at most 2000 characters (`perf/budgets.toml`). A link over
//! 8000 characters is refused with a hint; it counts as 8001 here, so a growing corpus cannot
//! hide behind refusals. Requests the scanner refuses (credentials in a script, for example)
//! have no link and are counted apart. `scripts/perf/share.mjs` runs this with
//! `RT_PERF_OUT=<dir>` and reads `<dir>/share.json`.

use std::path::{Path, PathBuf};

use rt_app::share::{MAX_LINK_LEN, ShareError, ShareOptions, build};
use rt_core::model::{Item, RequestSpec};
use rt_format::{FileSet, read_collection};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

fn requests(items: &[Item], out: &mut Vec<RequestSpec>) {
    for item in items {
        match item {
            Item::Request(r) => out.push(r.spec.clone()),
            Item::Folder(f) => requests(&f.items, out),
        }
    }
}

#[test]
fn share_url_size_of_the_bruno_corpus_is_measured() {
    let mut dirs: Vec<_> = std::fs::read_dir(corpus_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut specs = Vec::new();
    for dir in dirs {
        if let Ok(set) = FileSet::from_dir(&dir)
            && let Ok(doc) = read_collection(&set)
        {
            requests(&doc.model().items, &mut specs);
        }
    }
    // A typical address of the web app: its length is part of every link.
    let options = ShareOptions {
        base: Some("https://app.example.test/".into()),
        ..ShareOptions::default()
    };
    let mut lengths = Vec::new();
    let mut refused = 0usize;
    for spec in &specs {
        match build(&spec.name, spec, None, &options) {
            Ok(link) => lengths.push(link.length),
            Err(ShareError::TooLong(_)) => lengths.push(MAX_LINK_LEN + 1),
            Err(ShareError::SecretsFound(_)) => refused += 1,
            Err(e) => panic!("{}: {e}", spec.name),
        }
    }
    assert!(lengths.len() > 200, "{} links", lengths.len());
    lengths.sort_unstable();
    let p95 = lengths[(lengths.len() * 95).div_ceil(100) - 1];
    let median = lengths[lengths.len() / 2];
    let max = *lengths.last().unwrap();
    println!(
        "share_url_size: p95 {p95} chars (median {median}, max {max}) over {} links, {refused} refused by the scanner",
        lengths.len()
    );
    if let Some(dir) = std::env::var_os("RT_PERF_OUT") {
        let json = serde_json::json!({
            "share_url_size": p95, "median": median, "max": max,
            "links": lengths.len(), "refused": refused,
        });
        std::fs::write(
            Path::new(&dir).join("share.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }
}
