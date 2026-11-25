// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paging rules in collection files (wave 18, strand C): the companion `<request>.paging.json`
//! and the collection default `extensions.requesttailor.paging`. Both are stable byte for byte,
//! and writing the default leaves the rest of `opencollection.yml` as it is.

use rt_core::paging::{CursorParam, CursorPlace, PagingRule, PagingSpec};
use rt_format::paging_config::{
    companion_path, configured, is_companion, read_companion, write_companion,
};
use rt_format::{COLLECTION_FILE, FileSet, read_collection};

const HEAD: &str = "opencollection: 1.0.0\ninfo:\n  name: shop\n";
const BASE: &str = "opencollection: 1.0.0\ninfo:\n  name: shop\nextensions:\n  requesttailor:\n    http3: true # keep\n    paging:\n      rule: json-url\n      next: $.next\n      merge: $.items\n";

fn set(yaml: &str) -> FileSet {
    FileSet {
        files: [(COLLECTION_FILE.to_owned(), yaml.as_bytes().to_vec())].into(),
    }
}

fn text(s: &FileSet) -> String {
    String::from_utf8(s.files[COLLECTION_FILE].clone()).unwrap()
}

fn json_url() -> PagingSpec {
    PagingSpec {
        rule: PagingRule::JsonUrl {
            next: "$.next".into(),
        },
        merge: Some("$.items".into()),
    }
}

fn cursor() -> PagingSpec {
    PagingSpec {
        rule: PagingRule::Cursor {
            next: "$.meta.cursor".into(),
            param: CursorParam {
                place: CursorPlace::Query,
                name: "after".into(),
            },
        },
        merge: None,
    }
}

#[test]
fn the_companion_has_fixed_keys_in_a_stable_order() {
    let text = write_companion(Some(&json_url()));
    assert_eq!(
        text,
        "{\n  \"version\": 1,\n  \"rule\": {\n    \"kind\": \"json-url\",\n    \"next\": \"$.next\"\n  },\n  \"merge\": \"$.items\"\n}\n"
    );
    let cursor_text = write_companion(Some(&cursor()));
    assert!(
        cursor_text.contains(
            "\"kind\": \"cursor\",\n    \"next\": \"$.meta.cursor\",\n    \"param\": {\n      \"place\": \"query\",\n      \"name\": \"after\"\n    }"
        ),
        "{cursor_text}"
    );
    assert!(!cursor_text.contains("merge"), "no merge, no key");
    assert_eq!(
        write_companion(Some(&PagingSpec {
            rule: PagingRule::LinkHeader,
            merge: None
        })),
        "{\n  \"version\": 1,\n  \"rule\": {\n    \"kind\": \"link-header\"\n  }\n}\n"
    );
}

#[test]
fn the_companion_round_trips_byte_for_byte() {
    for spec in [
        json_url(),
        cursor(),
        PagingSpec {
            rule: PagingRule::LinkHeader,
            merge: Some("$".into()),
        },
    ] {
        let text = write_companion(Some(&spec));
        let back = read_companion(&text).unwrap().expect("a rule");
        assert_eq!(back, spec);
        assert_eq!(write_companion(Some(&back)), text);
    }
}

#[test]
fn no_rule_is_an_empty_object() {
    assert_eq!(write_companion(None), "{}\n");
    assert_eq!(read_companion("{}\n").unwrap(), None);
    assert_eq!(read_companion("  {}  ").unwrap(), None);
}

#[test]
fn a_damaged_or_foreign_companion_is_refused_with_a_reason() {
    for (bad, why) in [
        ("not json", "JSON"),
        (
            "{\"version\": 2, \"rule\": {\"kind\": \"link-header\"}}",
            "version",
        ),
        (
            "{\"version\": 1, \"rule\": {\"kind\": \"telepathy\"}}",
            "telepathy",
        ),
        (
            "{\"version\": 1, \"rule\": {\"kind\": \"link-header\"}, \"extra\": 1}",
            "extra",
        ),
        ("[]", "object"),
    ] {
        let err = read_companion(bad).unwrap_err().to_string();
        assert!(err.contains(why), "{bad:?} gave {err:?}");
    }
}

#[test]
fn the_companion_sits_next_to_the_request_file() {
    assert_eq!(companion_path("users/list.yml"), "users/list.paging.json");
    assert_eq!(companion_path("list.yml"), "list.paging.json");
    assert_eq!(companion_path("a.b/c.d.yml"), "a.b/c.d.paging.json");
    assert!(is_companion("users/list.paging.json"));
    assert!(!is_companion("users/list.yml"));
}

#[test]
fn reads_the_collection_default() {
    let doc = read_collection(&set(BASE)).unwrap();
    assert_eq!(doc.paging_config(), Some(json_url()));
    assert_eq!(configured(&set(BASE)), Some(json_url()));
    let none = read_collection(&set(HEAD)).unwrap();
    assert_eq!(none.paging_config(), None);
}

#[test]
fn a_cursor_default_reads_back() {
    let mut files = set(HEAD);
    let mut doc = read_collection(&files).unwrap();
    doc.set_paging_config(Some(&cursor())).unwrap();
    files = doc.write();
    let y = text(&files);
    assert!(y.contains("paging:\n      rule: cursor\n      next: $.meta.cursor\n      param:\n        place: query\n        name: after\n"), "{y}");
    assert_eq!(configured(&files), Some(cursor()));
}

#[test]
fn writing_the_default_touches_only_its_block() {
    let mut doc = read_collection(&set(BASE)).unwrap();
    doc.set_paging_config(Some(&cursor())).unwrap();
    let written = text(&doc.write());
    assert!(written.contains("http3: true # keep"), "{written}");
    assert!(written.starts_with(&BASE[..BASE.find("    paging:").unwrap()]));
    // Setting what is there writes nothing.
    let mut same = read_collection(&set(BASE)).unwrap();
    same.set_paging_config(Some(&json_url())).unwrap();
    assert_eq!(text(&same.write()), BASE);
}

#[test]
fn removing_the_default_removes_empty_parents() {
    let mut doc = read_collection(&set(
        "opencollection: 1.0.0\ninfo:\n  name: shop\nextensions:\n  requesttailor:\n    paging:\n      rule: link-header\n",
    ))
    .unwrap();
    doc.set_paging_config(None).unwrap();
    assert_eq!(text(&doc.write()), HEAD);
}

#[test]
fn a_file_without_the_block_is_unchanged_by_reading() {
    let doc = read_collection(&set(HEAD)).unwrap();
    assert_eq!(text(&doc.write()), HEAD);
}

#[test]
fn an_odd_default_reads_as_none() {
    let odd = format!("{HEAD}extensions:\n  requesttailor:\n    paging:\n      rule: telepathy\n");
    assert_eq!(read_collection(&set(&odd)).unwrap().paging_config(), None);
    let missing = format!(
        "{HEAD}extensions:\n  requesttailor:\n    paging:\n      rule: cursor\n      next: $.c\n"
    );
    assert_eq!(
        read_collection(&set(&missing)).unwrap().paging_config(),
        None,
        "a cursor needs its parameter"
    );
}
