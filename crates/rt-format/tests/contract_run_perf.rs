// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget of the run wizard: with a spec of 1000 operations parsed once, `requirements` for a
//! request that matches an operation late in the list takes under 5 ms (median of 50 runs, in
//! the debug build CI runs). `validate_response` for a typical body is timed and printed.

use rt_core::model::{KeyValue, RequestSpec};
use rt_format::contract::{
    ParsedSpec, RequestFacts, ResponseFacts, requirements, validate_response,
};
use serde_json::{Map, Value, json};
use web_time::{Duration, Instant};

const OPERATIONS: usize = 1000;
const RUNS: usize = 50;
const BUDGET: Duration = Duration::from_millis(5);

fn spec() -> Vec<u8> {
    let mut paths = Map::new();
    for i in 0..OPERATIONS / 2 {
        let item = json!({
            "get": {
                "summary": format!("Get item {i}"),
                "parameters": [
                    { "name": "id", "in": "path", "required": true, "schema": { "type": "integer", "minimum": 1 } },
                    { "name": "limit", "in": "query", "schema": { "type": "integer", "maximum": 100 } },
                    { "name": "fields", "in": "query", "schema": { "type": "array", "items": { "type": "string" } } },
                    { "name": "X-Trace", "in": "header", "required": true, "schema": { "type": "string", "format": "uuid" } }
                ],
                "responses": { "200": { "description": "ok", "content": { "application/json": {
                    "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Item" } }
                } } } }
            },
            "put": {
                "parameters": [
                    { "name": "id", "in": "path", "required": true, "schema": { "type": "integer" } }
                ],
                "requestBody": { "required": true, "content": { "application/json": {
                    "schema": { "$ref": "#/components/schemas/Item" }
                } } },
                "responses": { "204": { "description": "ok" } }
            }
        });
        paths.insert(format!("/area{}/resource{i}/{{id}}", i % 7), item);
    }
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "Big", "version": "1" },
        "servers": [{ "url": "https://api.example.com/v1" }],
        "components": { "schemas": { "Item": {
            "type": "object",
            "required": ["id", "name", "price"],
            "properties": {
                "id": { "type": "integer", "minimum": 1 },
                "name": { "type": "string", "maxLength": 80 },
                "price": { "type": "number", "minimum": 0 },
                "created": { "type": "string", "format": "date-time" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "state": { "type": "string", "enum": ["new", "used"] },
                "code": { "type": "string", "pattern": "^[A-Z]{3}-[0-9]+$" }
            }
        } } },
        "paths": paths
    });
    serde_json::to_vec(&spec).unwrap()
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

#[test]
fn requirements_and_response_check_are_fast() {
    let bytes = spec();
    let started = Instant::now();
    let spec = ParsedSpec::parse(&bytes).unwrap();
    let parse = started.elapsed();
    assert_eq!(spec.operations(), OPERATIONS);

    let last = OPERATIONS / 2 - 1;
    let request = RequestSpec {
        name: "late".into(),
        method: "GET".into(),
        url: format!(
            "{{{{baseUrl}}}}/area{}/resource{last}/0?limit=500&fields=a,b",
            last % 7
        ),
        headers: vec![KeyValue::new("X-Trace", "nope")],
        ..RequestSpec::default()
    };
    let facts = RequestFacts {
        request: &request,
        variables: &[],
    };
    let mut times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        let r = requirements(&spec, &facts).expect("matches");
        times.push(started.elapsed());
        assert_eq!(r.fields.iter().filter(|f| f.problem.is_some()).count(), 3);
    }
    let req = median(times);

    let items: Vec<Value> = (1..=100)
        .map(|i| {
            json!({
                "id": i, "name": format!("Item {i}"), "price": 9.5,
                "created": "2024-01-31T12:00:00Z", "tags": ["a", "b"],
                "state": "new", "code": format!("ABC-{i}")
            })
        })
        .collect();
    let body = serde_json::to_vec(&items).unwrap();
    let headers = vec![("Content-Type".to_owned(), "application/json".to_owned())];
    let response = ResponseFacts {
        status: 200,
        headers: &headers,
        body: Some(&body),
    };
    let mut times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        let c = validate_response(&spec, &request, &response).expect("matches");
        times.push(started.elapsed());
        assert!(c.differences.is_empty(), "{:?}", c.differences);
    }
    let resp = median(times);

    println!(
        "contract run: parse {OPERATIONS} operations {parse:?}; requirements median {req:?} \
         (budget {BUDGET:?}); validate_response ({} KB, 100 items) median {resp:?}",
        body.len() / 1024
    );
    assert!(
        req < BUDGET,
        "requirements took {req:?} (median of {RUNS}), budget {BUDGET:?}"
    );
}
