// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests (wave 17, strand B), the file side: a request with a `matchesSnapshot`
//! assertion reads and writes byte-identically, adding the assertion changes only that
//! request file, and `*.snap.json` companion files are not part of the collection.

use rt_format::{COLLECTION_FILE, FileSet, read_collection};

fn set(snap: Option<&str>) -> FileSet {
    let mut list = vec![
        (
            COLLECTION_FILE.to_owned(),
            "opencollection: \"1.0.0\"\ninfo:\n  name: snaps\n".to_owned(),
        ),
        (
            "users/get.yml".to_owned(),
            "info:\n  name: Get user\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://x/users\n\nruntime:\n  assertions:\n    - expression: res\n      operator: matchesSnapshot\n      value: get.snap.json\n".to_owned(),
        ),
        (
            "users/other.yml".to_owned(),
            "info:\n  name: Other\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: http://x/other\n".to_owned(),
        ),
    ];
    if let Some(text) = snap {
        list.push(("users/get.snap.json".to_owned(), text.to_owned()));
    }
    FileSet {
        files: list.into_iter().map(|(p, t)| (p, t.into_bytes())).collect(),
    }
}

#[test]
fn a_request_with_the_snapshot_assertion_round_trips_byte_for_byte() {
    let input = set(Some("{}"));
    let doc = read_collection(&input).unwrap();
    let spec = &doc.model().request("users/get.yml").unwrap().request.spec;
    assert_eq!(spec.assertions.len(), 1);
    assert_eq!(spec.assertions[0].operator, "matchesSnapshot");
    assert_eq!(spec.assertions[0].expression, "res");
    assert_eq!(spec.assertions[0].value, "get.snap.json");
    let written = doc.write();
    for (path, bytes) in &input.files {
        if path.ends_with(".yml") {
            assert_eq!(written.files.get(path), Some(bytes), "{path}");
        }
    }
}

#[test]
fn the_companion_file_is_not_a_collection_file() {
    let with = read_collection(&set(Some("{ \"not\": \"yaml: [\" }"))).unwrap();
    let without = read_collection(&set(None)).unwrap();
    assert_eq!(with.model(), without.model());
    // Writing does not drop or invent it: the writer returns only the document's files.
    assert!(!with.write().files.contains_key("users/get.snap.json"));
}

#[test]
fn adding_the_assertion_changes_only_that_request_and_undoing_restores_the_bytes() {
    let input = set(None);
    let base = read_collection(&input).unwrap();
    let original = base.write();
    let mut spec = base
        .model()
        .request("users/other.yml")
        .unwrap()
        .request
        .spec
        .clone();
    spec.assertions.push(rt_core::model::Assertion {
        expression: "res".into(),
        operator: rt_core::assert::MATCHES_SNAPSHOT.into(),
        value: "other.snap.json".into(),
        description: None,
        enabled: true,
    });
    let mut doc = base.clone();
    doc.set_request("users/other.yml", &spec).unwrap();
    let written = doc.write();
    let differing: Vec<&String> = written
        .files
        .iter()
        .filter(|(p, b)| original.files.get(*p) != Some(b))
        .map(|(p, _)| p)
        .collect();
    assert_eq!(differing, ["users/other.yml"]);
    let text = String::from_utf8(written.files["users/other.yml"].clone()).unwrap();
    assert!(text.contains("operator: matchesSnapshot"), "{text}");
    assert!(text.contains("value: other.snap.json"), "{text}");
    let back = read_collection(&written).unwrap();
    assert_eq!(
        back.model()
            .request("users/other.yml")
            .unwrap()
            .request
            .spec,
        spec
    );
    spec.assertions.clear();
    doc.set_request("users/other.yml", &spec).unwrap();
    assert_eq!(doc.write(), original);
}
