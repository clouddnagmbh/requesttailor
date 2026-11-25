// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.lint` (wave 18, strand F): rule levels, `ignore` patterns and
//! `min_score`. Reading and writing leave everything else in `opencollection.yml` byte for byte.

use std::collections::BTreeMap;

use rt_format::lint_config::LintConfig;
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

const HEAD: &str = "opencollection: 1.0.0\ninfo:\n  name: shop\n";
const BASE: &str = "opencollection: 1.0.0\ninfo:\n  name: shop\nextensions:\n  requesttailor:\n    lint:\n      rules:\n        lint/no-tests: off # not yet\n        lint/duplicate-name: warn\n      ignore:\n        - \"legacy/*\"\n        - lint/hard-coded-host:users/get.yml\n      min_score: 80\n    http3: true\n";

fn set(yaml: &str) -> FileSet {
    FileSet {
        files: [(COLLECTION_FILE.to_owned(), yaml.as_bytes().to_vec())].into(),
    }
}

fn text(s: &FileSet) -> String {
    String::from_utf8(s.files[COLLECTION_FILE].clone()).unwrap()
}

fn rules(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn reads_the_settings() {
    let doc = read_collection(&set(BASE)).unwrap();
    let config = doc.lint_config();
    assert_eq!(
        config.rules,
        rules(&[("lint/no-tests", "off"), ("lint/duplicate-name", "warn")])
    );
    assert_eq!(
        config.ignore,
        vec!["legacy/*", "lint/hard-coded-host:users/get.yml"]
    );
    assert_eq!(config.min_score, Some(80));
    assert_eq!(rt_format::lint_config::configured(&set(BASE)), config);
}

#[test]
fn a_file_without_settings_gives_the_empty_config() {
    let doc = read_collection(&set(HEAD)).unwrap();
    assert_eq!(doc.lint_config(), LintConfig::default());
}

#[test]
fn odd_values_are_left_out_not_guessed() {
    let odd = format!(
        "{HEAD}extensions:\n  requesttailor:\n    lint:\n      rules: nope\n      ignore: [a, 1]\n      min_score: 250\n"
    );
    let config = read_collection(&set(&odd)).unwrap().lint_config();
    assert!(config.rules.is_empty());
    assert_eq!(config.min_score, None);
    let odd = format!("{HEAD}extensions:\n  requesttailor:\n    lint:\n      min_score: high\n");
    assert_eq!(
        read_collection(&set(&odd)).unwrap().lint_config().min_score,
        None
    );
}

#[test]
fn reading_and_writing_keep_the_file_byte_for_byte() {
    let files = set(BASE);
    let mut doc = read_collection(&files).unwrap();
    assert_eq!(doc.write(), files);
    // Setting what is there changes nothing.
    let same = doc.lint_config();
    doc.set_lint_config(&same).unwrap();
    assert_eq!(doc.write(), files);
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
}

#[test]
fn writes_a_new_block_after_a_bare_file() {
    let mut doc = read_collection(&set(HEAD)).unwrap();
    let config = LintConfig {
        rules: rules(&[("lint/orphan-variable", "off")]),
        ignore: vec!["*.draft.yml".into()],
        min_score: Some(70),
    };
    doc.set_lint_config(&config).unwrap();
    let written = doc.write();
    assert_eq!(
        text(&written),
        format!(
            "{HEAD}\nextensions:\n  requesttailor:\n    lint:\n      rules:\n        lint/orphan-variable: off\n      ignore:\n        - \"*.draft.yml\"\n      min_score: 70\n"
        )
    );
    // Reads back, and writing again gives the same bytes.
    let again = read_collection(&written).unwrap();
    assert!(again.issues().is_empty());
    assert_eq!(again.lint_config(), config);
    assert_eq!(again.write(), written);
}

#[test]
fn an_edit_keeps_the_other_extensions_and_the_rest_of_the_file() {
    let mut doc = read_collection(&set(BASE)).unwrap();
    let mut config = doc.lint_config();
    config.ignore.push("lint/no-tests:ping.yml".into());
    doc.set_lint_config(&config).unwrap();
    let out = text(&doc.write());
    assert!(out.starts_with(HEAD), "{out}");
    assert!(out.contains("    http3: true\n"), "{out}");
    assert!(out.contains("lint/no-tests:ping.yml"), "{out}");
    assert_eq!(doc.lint_config(), config);
}

#[test]
fn an_empty_config_removes_the_block_and_empty_parents() {
    let mut doc = read_collection(&set(&format!(
        "{HEAD}extensions:\n  requesttailor:\n    lint:\n      min_score: 50\n"
    )))
    .unwrap();
    doc.set_lint_config(&LintConfig::default()).unwrap();
    assert_eq!(text(&doc.write()), HEAD);
}

#[test]
fn the_other_writers_of_the_file_leave_the_block_alone() {
    let mut doc = read_collection(&set(BASE)).unwrap();
    doc.set_contract_spec("api/openapi.yaml").unwrap();
    let out = text(&doc.write());
    assert!(out.contains("      min_score: 80\n"), "{out}");
    assert!(out.contains("lint/hard-coded-host:users/get.yml"), "{out}");
}

#[test]
fn min_score_beyond_100_is_refused() {
    let mut doc = read_collection(&set(HEAD)).unwrap();
    let config = LintConfig {
        min_score: Some(101),
        ..LintConfig::default()
    };
    assert!(doc.set_lint_config(&config).is_err());
    assert_eq!(doc.write(), set(HEAD));
}
