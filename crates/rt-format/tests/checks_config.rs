// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `extensions.requesttailor.checks` (wave 18, strand B): the rules a collection switches off.
//! Reading and writing leave everything else in `opencollection.yml` byte for byte.

use rt_format::checks_config::ChecksConfig;
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

const HEAD: &str = "opencollection: 1.0.0\ninfo:\n  name: shop\n";
const BASE: &str = "opencollection: 1.0.0\ninfo:\n  name: shop\nextensions:\n  requesttailor:\n    http3: true\n    checks:\n      ignore:\n        - chk/hsts-missing # known\n        - chk/stack-trace\n";

fn set(yaml: &str) -> FileSet {
    FileSet {
        files: [(COLLECTION_FILE.to_owned(), yaml.as_bytes().to_vec())].into(),
    }
}

fn text(s: &FileSet) -> String {
    String::from_utf8(s.files[COLLECTION_FILE].clone()).unwrap()
}

fn ids(list: &[&str]) -> ChecksConfig {
    ChecksConfig {
        ignore: list.iter().map(|s| (*s).to_owned()).collect(),
    }
}

#[test]
fn reads_the_list_and_ignores_odd_values() {
    let doc = read_collection(&set(BASE)).unwrap();
    assert_eq!(
        doc.checks_config(),
        ids(&["chk/hsts-missing", "chk/stack-trace"])
    );
    assert_eq!(
        rt_format::checks_config::configured(&set(BASE)),
        doc.checks_config()
    );
    let odd = format!("{HEAD}extensions:\n  requesttailor:\n    checks:\n      ignore: nope\n");
    assert!(
        read_collection(&set(&odd))
            .unwrap()
            .checks_config()
            .ignore
            .is_empty()
    );
    assert!(
        read_collection(&set(HEAD))
            .unwrap()
            .checks_config()
            .ignore
            .is_empty()
    );
}

#[test]
fn reading_and_setting_the_same_keep_the_file_byte_for_byte() {
    let files = set(BASE);
    let mut doc = read_collection(&files).unwrap();
    assert_eq!(doc.write(), files);
    let same = doc.checks_config();
    doc.set_checks_config(&same).unwrap();
    assert_eq!(doc.write(), files);
}

#[test]
fn writes_a_new_block_and_it_reads_back() {
    let mut doc = read_collection(&set(HEAD)).unwrap();
    doc.set_checks_config(&ids(&["chk/cors-credentials"]))
        .unwrap();
    let written = doc.write();
    assert_eq!(
        text(&written),
        format!(
            "{HEAD}\nextensions:\n  requesttailor:\n    checks:\n      ignore:\n        - chk/cors-credentials\n"
        )
    );
    let again = read_collection(&written).unwrap();
    assert!(again.issues().is_empty(), "{:?}", again.issues());
    assert_eq!(again.checks_config(), ids(&["chk/cors-credentials"]));
    assert_eq!(again.write(), written);
}

#[test]
fn an_edit_keeps_the_other_extensions_and_an_empty_list_removes_the_block() {
    let mut doc = read_collection(&set(BASE)).unwrap();
    let mut config = doc.checks_config();
    config.ignore.push("chk/nosniff-missing".into());
    doc.set_checks_config(&config).unwrap();
    let out = text(&doc.write());
    assert!(
        out.starts_with(HEAD) && out.contains("    http3: true\n"),
        "{out}"
    );
    assert!(out.contains("chk/nosniff-missing"), "{out}");
    doc.set_checks_config(&ChecksConfig::default()).unwrap();
    assert_eq!(
        text(&doc.write()),
        format!("{HEAD}extensions:\n  requesttailor:\n    http3: true\n")
    );
}

#[test]
fn a_bad_id_is_refused() {
    let mut doc = read_collection(&set(HEAD)).unwrap();
    assert!(doc.set_checks_config(&ids(&["a\nb"])).is_err());
    assert!(doc.set_checks_config(&ids(&[" "])).is_err());
    assert_eq!(text(&doc.write()), HEAD);
}
