// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 7, strand C: every declarative assertion of the Bruno corpus evaluates (known
//! operator, parsable expression) and passes against a response built to match it, and
//! fails against one that does not.

use std::path::{Path, PathBuf};

use rt_core::assert::evaluate;
use rt_core::model::{Assertion, Item};
use rt_core::script::ScriptResponse;
use rt_core::vars::{Scope, VarLayer, VarSource};
use rt_format::{FileSet, read_collection};
use serde_json::{Map, Value};

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

fn assertions(items: &[Item], out: &mut Vec<(String, Vec<Assertion>)>) {
    for item in items {
        match item {
            Item::Folder(f) => assertions(&f.items, out),
            Item::Request(r) if !r.spec.assertions.is_empty() => {
                out.push((r.path.clone(), r.spec.assertions.clone()));
            }
            Item::Request(_) => {}
        }
    }
}

/// Bruno's reading of an expected value, for building the matching response.
fn value_of(raw: &str) -> Value {
    match raw.trim() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        s => s
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(s.to_owned())),
    }
}

/// Sets `a.b.c` in `body` (the corpus uses dotted paths only).
fn set_path(body: &mut Map<String, Value>, path: &str, value: Value) {
    let mut keys = path.split('.').peekable();
    let mut current = body;
    while let Some(key) = keys.next() {
        if keys.peek().is_none() {
            current.insert(key.to_owned(), value);
            return;
        }
        current = current
            .entry(key.to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("an object on the way");
    }
}

/// A response (and variables) on which every assertion of the list holds.
fn matching(list: &[Assertion]) -> (ScriptResponse, Scope) {
    let mut status = 200;
    let mut body = Map::new();
    let mut vars = Vec::new();
    for a in list.iter().filter(|a| a.enabled) {
        assert_eq!(
            a.operator, "eq",
            "the corpus only uses eq; extend this builder"
        );
        if a.expression == "res.status" {
            status = a.value.trim().parse().expect("a status code");
        } else if let Some(path) = a.expression.strip_prefix("res.body.") {
            set_path(&mut body, path, value_of(&a.value));
        } else {
            vars.push((a.expression.clone(), a.value.clone(), false));
        }
    }
    let response = ScriptResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: serde_json::to_vec(&Value::Object(body)).unwrap(),
        time_ms: 5,
        url: "http://127.0.0.1/".into(),
    };
    let scope = Scope {
        layers: vec![VarLayer {
            source: VarSource::Runtime,
            vars,
        }],
    };
    (response, scope)
}

fn collections() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(corpus())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("opencollection.yml").is_file())
        .collect();
    out.sort();
    out
}

fn read(root: &Path) -> Vec<(String, Vec<Assertion>)> {
    let files = FileSet::from_dir(root).unwrap();
    let doc = read_collection(&files).unwrap();
    let mut out = Vec::new();
    assertions(&doc.model().items, &mut out);
    out
}

#[test]
fn corpus_assertions_pass_on_matching_responses_and_fail_otherwise() {
    let mut files = 0;
    let mut count = 0;
    for root in collections() {
        for (path, list) in read(&root) {
            files += 1;
            let (response, scope) = matching(&list);
            let results = evaluate(&list, &response, &scope);
            assert_eq!(
                results.len(),
                list.iter().filter(|a| a.enabled).count(),
                "{path}"
            );
            for r in &results {
                count += 1;
                assert!(r.passed, "{}/{path}: {r:?}", root.display());
                assert_eq!(r.error, None, "{path}: {r:?}");
            }
            // A different status and an empty body: every assertion fails, none is unknown.
            let other = ScriptResponse {
                status: 599,
                body: b"{}".to_vec(),
                ..response
            };
            for r in evaluate(&list, &other, &Scope::default()) {
                assert!(!r.passed, "{path}: {r:?}");
                let e = r.error.unwrap_or_default();
                assert!(!e.contains("unknown"), "{path}: {e}");
            }
        }
    }
    // testdata/collections/bruno: 38 request files with assertions (see CLAUDE.md).
    assert_eq!(files, 38, "request files with assertions");
    assert!(count >= 90, "{count} assertions");
}
