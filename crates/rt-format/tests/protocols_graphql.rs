// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 8, strand B, point 4): GraphQL items (`type: graphql`): reading the
//! corpus, edits with the read-back rule, body variants, switching between HTTP and GraphQL,
//! new files.

use std::path::{Path, PathBuf};

use rt_core::model::{Auth, Body, KeyValue, Protocol, RawKind, RequestSpec, Settings};
use rt_format::{CollectionDoc, FileSet, read_collection};

fn corpus(name: &str) -> CollectionDoc {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/collections/bruno")
        .join(name);
    read_collection(&FileSet::from_dir(&dir).unwrap()).unwrap()
}

fn files(list: &[(&str, &str)]) -> FileSet {
    FileSet {
        files: list
            .iter()
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect(),
    }
}

fn text(set: &FileSet, path: &str) -> String {
    String::from_utf8(set.files[path].clone()).unwrap()
}

fn spec(doc: &CollectionDoc, path: &str) -> RequestSpec {
    doc.model().request(path).unwrap().request.spec.clone()
}

fn gql(query: &str, variables: &str) -> Body {
    Body::GraphQl {
        query: query.into(),
        variables: variables.into(),
    }
}

const COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: q\n";

#[test]
fn corpus_graphql_items_read_as_graphql_bodies() {
    let doc = corpus("graphql-query-builder");
    let s = spec(&doc, "test-graphql.yml");
    assert_eq!(s.method, "POST");
    assert_eq!(s.url, "http://localhost:8081/api/graphql");
    assert_eq!(s.body, gql("", "{}"));
    assert_eq!(s.auth, Auth::Inherit);
    assert_eq!(s.protocol, Protocol::Http);
    assert_eq!(s.settings.timeout_ms, Some(0));
    assert_eq!(s.settings.encode_url, Some(true));
    assert_eq!(spec(&doc, "deep-nesting-echo.yml").seq, Some(2.0));
}

#[test]
fn editing_the_query_and_back_in_a_corpus_item() {
    let mut doc = corpus("graphql-query-builder");
    let original = doc.write();
    let before = spec(&doc, "test-graphql.yml");
    let mut after = before.clone();
    after.body = gql("query Books {\n  books {\n    title\n  }\n}", "{\"n\": 1}");
    after.headers.push(KeyValue::new("x-a", "1"));
    after.settings = Settings {
        timeout_ms: Some(500),
        ..before.settings.clone()
    };
    doc.set_request("test-graphql.yml", &after).unwrap();
    let written = text(&doc.write(), "test-graphql.yml");
    assert!(
        written.contains("    query: |-\n      query Books {\n        books {\n          title\n        }\n      }\n"),
        "{written}"
    );
    assert!(
        written.contains("  headers:\n    - name: x-a\n"),
        "{written}"
    );
    assert!(written.contains("  timeout: 500\n"), "{written}");
    let reread = read_collection(&doc.write()).unwrap();
    assert_eq!(spec(&reread, "test-graphql.yml"), after);
    doc.set_request("test-graphql.yml", &before).unwrap();
    let back = doc.write();
    assert_eq!(
        spec(&read_collection(&back).unwrap(), "test-graphql.yml"),
        before
    );
    // Other files never change.
    for (path, bytes) in &original.files {
        if path != "test-graphql.yml" {
            assert_eq!(back.files.get(path), Some(bytes), "{path}");
        }
    }
}

#[test]
fn only_the_changed_body_key_is_rewritten() {
    let file = "info:\n  name: q\n  type: graphql\n  seq: 1\n\ngraphql:\n  method: POST\n  url: https://api.example/graphql\n  body:\n    # the query\n    query: \"{ a }\"\n    variables: \"{}\"\n  auth: inherit\n";
    let mut doc = read_collection(&files(&[
        ("opencollection.yml", COLLECTION),
        ("q.yml", file),
    ]))
    .unwrap();
    let mut s = spec(&doc, "q.yml");
    s.body = gql("{ a }", "{\"x\": 1}");
    doc.set_request("q.yml", &s).unwrap();
    assert_eq!(
        text(&doc.write(), "q.yml"),
        file.replace("variables: \"{}\"", "variables: '{\"x\": 1}'")
    );
}

#[test]
fn the_selected_body_variant_is_read_and_edited() {
    let file = "info:\n  name: q\n  type: graphql\n\ngraphql:\n  url: https://api.example/graphql\n  body:\n    - title: one\n      body:\n        query: \"{ one }\"\n    - title: two\n      selected: true\n      body:\n        query: \"{ two }\"\n        variables: \"{}\"\n";
    let mut doc = read_collection(&files(&[
        ("opencollection.yml", COLLECTION),
        ("q.yml", file),
    ]))
    .unwrap();
    let mut s = spec(&doc, "q.yml");
    assert_eq!(s.method, "POST");
    assert_eq!(s.body, gql("{ two }", "{}"));
    s.body = gql("{ three }", "{}");
    doc.set_request("q.yml", &s).unwrap();
    assert_eq!(
        text(&doc.write(), "q.yml"),
        file.replace("query: \"{ two }\"", "query: \"{ three }\"")
    );
}

#[test]
fn switching_between_http_and_graphql_changes_the_item_type() {
    let http = "info:\n  name: q\n  type: http\n  seq: 1\n\nhttp:\n  method: POST\n  url: https://api.example/graphql\n  headers:\n    - name: x-a\n      value: \"1\"\n  body:\n    type: json\n    data: \"{}\"\n  auth: inherit\n\nsettings:\n  encodeUrl: true\n";
    let mut doc = read_collection(&files(&[
        ("opencollection.yml", COLLECTION),
        ("q.yml", http),
    ]))
    .unwrap();
    let before = spec(&doc, "q.yml");
    let mut s = before.clone();
    s.body = gql("{ a }", "");
    doc.set_request("q.yml", &s).unwrap();
    let written = text(&doc.write(), "q.yml");
    assert_eq!(
        written,
        "info:\n  name: q\n  type: graphql\n  seq: 1\n\ngraphql:\n  method: POST\n  url: https://api.example/graphql\n  headers:\n    - name: x-a\n      value: \"1\"\n  body:\n    query: \"{ a }\"\n    variables: \"\"\n  auth: inherit\n\nsettings:\n  encodeUrl: true\n"
    );
    assert_eq!(spec(&read_collection(&doc.write()).unwrap(), "q.yml"), s);
    // And back to JSON.
    doc.set_request("q.yml", &before).unwrap();
    assert_eq!(text(&doc.write(), "q.yml"), http);
    let mut raw = before.clone();
    raw.body = Body::Raw {
        kind: RawKind::Text,
        data: "x".into(),
    };
    doc.set_request("q.yml", &raw).unwrap();
    assert!(text(&doc.write(), "q.yml").contains("type: http"));
}

#[test]
fn new_graphql_requests_are_laid_out_like_bruno() {
    let mut doc = read_collection(&files(&[("opencollection.yml", COLLECTION)])).unwrap();
    let s = RequestSpec {
        name: "Books".into(),
        method: "POST".into(),
        url: "{{api}}/graphql".into(),
        body: gql("{\n  books {\n    title\n  }\n}", "{}"),
        auth: Auth::Inherit,
        ..RequestSpec::default()
    };
    let path = doc.add_request("", &s).unwrap();
    assert_eq!(
        text(&doc.write(), &path),
        "info:\n  name: Books\n  type: graphql\n  seq: 1\n\ngraphql:\n  method: POST\n  url: \"{{api}}/graphql\"\n  body:\n    query: |-\n      {\n        books {\n          title\n        }\n      }\n    variables: \"{}\"\n  auth: inherit\n"
    );
    let mut want = s.clone();
    want.seq = Some(1.0);
    assert_eq!(spec(&read_collection(&doc.write()).unwrap(), &path), want);
}
