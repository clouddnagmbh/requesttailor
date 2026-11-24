// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17: `rt_app::contract::stage_spec` writes exactly the spec the contract names (in the
//! collection folder or at the repository root), unsaved, and nothing else.

use rt_app::Workspace;
use rt_app::contract::stage_spec;
use rt_format::FileSet;

const SPEC: &str = "openapi: 3.0.3\ninfo:\n  title: Shop\n  version: 1.0.0\npaths:\n  /items:\n    get:\n      responses:\n        '200':\n          description: ok\n";
const NEWER: &str = "openapi: 3.0.3\ninfo:\n  title: Shop\n  version: 1.1.0\npaths:\n  /items:\n    get:\n      responses:\n        '200':\n          description: ok\n  /orders:\n    get:\n      responses:\n        '200':\n          description: ok\n";

const NEWER_JSON: &str = r#"{"openapi":"3.0.3","info":{"title":"Shop","version":"1.1.0"},"paths":{"/items":{"get":{"responses":{"200":{"description":"ok"}}}}}}"#;

fn workspace(extra: &[(&str, &str)], config: &str) -> Workspace {
    let mut files = vec![
        (
            "opencollection.yml".to_owned(),
            format!("opencollection: 1.0.0\ninfo:\n  name: Shop\n{config}"),
        ),
        (
            "items.yml".to_owned(),
            "info:\n  name: Items\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://x/items\n".to_owned(),
        ),
    ];
    files.extend(
        extra
            .iter()
            .map(|(p, t)| ((*p).to_owned(), (*t).to_owned())),
    );
    Workspace::open(FileSet {
        files: files
            .into_iter()
            .map(|(p, t)| (p, t.into_bytes()))
            .collect(),
    })
    .unwrap()
}

fn bytes_of(ws: &Workspace, path: &str) -> Vec<u8> {
    let changes = ws.clone().save().unwrap();
    changes
        .iter()
        .find(|c| c.path == path)
        .unwrap_or_else(|| panic!("{path} is not in the save: {changes:?}"))
        .text
        .clone()
        .into_bytes()
}

#[test]
fn a_spec_in_the_collection_folder_is_staged_unsaved() {
    let mut ws = workspace(&[("openapi.yaml", SPEC)], "");
    let path = stage_spec(&mut ws, "openapi.yaml", NEWER).unwrap();
    assert_eq!(path, "openapi.yaml");
    assert_eq!(ws.dirty(), ["openapi.yaml"]);
    assert_eq!(bytes_of(&ws, "openapi.yaml"), NEWER.as_bytes());
    // The saved file set still has the old text until `save`.
    assert_eq!(ws.files().files["openapi.yaml"], SPEC.as_bytes());
}

#[test]
fn a_spec_at_the_repository_root_is_staged_with_its_slash() {
    let mut ws = workspace(&[], "");
    let path = stage_spec(&mut ws, "/openapi.json", NEWER_JSON).unwrap();
    assert_eq!(path, "/openapi.json");
    assert_eq!(ws.dirty(), ["/openapi.json"]);
    let saved = ws.save().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].path, "/openapi.json");
    assert!(!saved[0].deleted);
    // Nothing leaks into the collection's file set, and the workspace is clean afterwards.
    assert!(!ws.files().files.keys().any(|p| p.starts_with('/')));
    assert!(ws.dirty().is_empty());
}

#[test]
fn a_configured_spec_is_the_only_one_that_may_be_written() {
    let config = "extensions:\n  requesttailor:\n    contract:\n      spec: api/spec.yaml\n";
    let mut ws = workspace(&[("api/spec.yaml", SPEC), ("openapi.yaml", SPEC)], config);
    assert_eq!(
        stage_spec(&mut ws, "api/spec.yaml", NEWER).unwrap(),
        "api/spec.yaml"
    );
    // The default candidate is not the contract's spec when one is configured.
    assert!(stage_spec(&mut ws, "openapi.yaml", NEWER).is_err());
    assert_eq!(ws.dirty(), ["api/spec.yaml"]);
    // A configured spec at the repository root.
    let config = "extensions:\n  requesttailor:\n    contract:\n      spec: /specs/shop.yaml\n";
    let mut ws = workspace(&[], config);
    assert_eq!(
        stage_spec(&mut ws, "/specs/shop.yaml", NEWER).unwrap(),
        "/specs/shop.yaml"
    );
}

#[test]
fn a_yml_spec_next_to_the_requests_is_staged_and_stays_a_spec() {
    // `openapi.yml` ends in `.yml`, the extension of collection files.
    let mut ws = workspace(&[("openapi.yml", SPEC)], "");
    assert_eq!(
        stage_spec(&mut ws, "openapi.yml", NEWER).unwrap(),
        "openapi.yml"
    );
    assert_eq!(ws.dirty(), ["openapi.yml"]);
    assert_eq!(bytes_of(&ws, "openapi.yml"), NEWER.as_bytes());
    // The collection still reads (the spec file is not a request) and lists no extra request.
    let ws2 = workspace(&[("openapi.yml", SPEC)], "");
    assert_eq!(format!("{:?}", ws.tree()), format!("{:?}", ws2.tree()));
}

#[test]
fn anything_but_the_contracts_spec_is_refused() {
    let mut ws = workspace(&[("openapi.yaml", SPEC)], "");
    for path in [
        "items.yml",
        "opencollection.yml",
        "other.yaml",
        "../openapi.yaml",
        "/../openapi.yaml",
        "/etc/passwd",
        "openapi.yaml/../items.yml",
        "",
        "C:\\openapi.yaml",
    ] {
        assert!(stage_spec(&mut ws, path, SPEC).is_err(), "{path:?}");
    }
    assert!(ws.dirty().is_empty(), "{:?}", ws.dirty());
}

#[test]
fn an_invalid_spec_is_refused_and_nothing_is_staged() {
    let mut ws = workspace(&[("openapi.yaml", SPEC)], "");
    for text in [
        "",
        "not: a spec",
        "openapi: 3.0.3\ninfo: [",
        "info:\n  name: a request\nhttp:\n  method: GET\n  url: http://x\n",
        "{\"openapi\": ",
    ] {
        assert!(
            stage_spec(&mut ws, "openapi.yaml", text).is_err(),
            "{text:?}"
        );
    }
    assert!(ws.dirty().is_empty());
    let huge = format!("{SPEC}# {}", "x".repeat(20 * 1024 * 1024));
    assert!(stage_spec(&mut ws, "openapi.yaml", &huge).is_err());
}

#[test]
fn staging_the_same_text_twice_keeps_one_pending_change() {
    let mut ws = workspace(&[("openapi.yaml", SPEC)], "");
    stage_spec(&mut ws, "openapi.yaml", NEWER).unwrap();
    stage_spec(&mut ws, "openapi.yaml", NEWER).unwrap();
    assert_eq!(ws.dirty(), ["openapi.yaml"]);
    // Staging the saved text again is no change.
    stage_spec(&mut ws, "openapi.yaml", SPEC).unwrap();
    assert!(ws.dirty().is_empty());
}
