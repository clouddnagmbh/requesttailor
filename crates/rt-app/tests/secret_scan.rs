// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The secret scanner over the test corpora (wave 5, strand E): the Bruno corpus
//! (`testdata/collections/bruno`) and the import corpus (`testdata/import`, sources and their
//! `.expected` collections). Every finding is listed, grouped by its reason, in
//! `secret_scan_corpus.txt`; the test fails when the scanner finds more or less, so a rule
//! change shows exactly which lines it adds or drops. `RT_UPDATE_SCAN_CORPUS=1` prints the
//! current findings in the file's format.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rt_app::secret_scan::{SecretFinding, scan, text_of};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

/// Findings over both corpora, paths relative to `testdata/`, `/`-separated.
fn corpus_findings() -> Vec<SecretFinding> {
    let base = testdata();
    let mut out = Vec::new();
    for corpus in ["collections/bruno", "import"] {
        let mut files = Vec::new();
        walk(&base.join(corpus), &mut files);
        assert!(files.len() > 100, "corpus {corpus} not found");
        for f in files {
            let bytes = std::fs::read(&f).unwrap();
            let Some(text) = text_of(&bytes) else {
                continue;
            };
            let rel: Vec<String> = f
                .strip_prefix(&base)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.extend(scan(&[(&rel.join("/"), text)]));
        }
    }
    out
}

#[test]
fn every_corpus_finding_is_listed_with_a_reason() {
    let findings = corpus_findings();
    let got: BTreeSet<String> = findings
        .iter()
        .map(|f| format!("{}:{} {}", f.path, f.line, f.kind))
        .collect();
    if std::env::var_os("RT_UPDATE_SCAN_CORPUS").is_some() {
        for line in &got {
            println!("{line}");
        }
    }
    let listed = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/secret_scan_corpus.txt"),
    )
    .unwrap();
    let want: BTreeSet<String> = listed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let new: Vec<&String> = got.difference(&want).collect();
    let gone: Vec<&String> = want.difference(&got).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "findings not in secret_scan_corpus.txt: {new:#?}\nlisted but no longer found: {gone:#?}"
    );
}

#[test]
fn corpus_snippets_are_masked_and_every_finding_has_a_hint() {
    for f in corpus_findings() {
        assert!(f.snippet.contains("***"), "{f:?}");
        assert!(f.hint.contains("rt-scan:allow"), "{f:?}");
        assert!(f.line >= 1);
    }
}
