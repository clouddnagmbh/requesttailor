// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Budget `schema_changes` (wave 18, strand H): the changes between two GraphQL schemas of 2000
//! types and 10 000 fields with 200 changes, and between two Protobuf versions of 500 messages,
//! each under 300 ms (median of 9, the slower of the two is the figure). The sides are in
//! memory, as the shells hand them over; the collection has 50 GraphQL requests, so naming the
//! requests of each change is part of the time. `scripts/perf/schemas.mjs` runs this in a
//! release build with `RT_PERF_OUT=<dir>` and reads `<dir>/schemas.json`.

use rt_format::FileSet;
use rt_format::contract::{SchemaKind, SchemaSources, schema_changes};

const RUNS: usize = 9;
const LIMIT_MS: f64 = 300.0;
const TYPES: usize = 2000;
const FIELDS: usize = 5;
const CHANGES: usize = 200;

fn graphql(changed: bool) -> String {
    let mut out = String::from("type Query { t0: T0 }\n");
    for t in 0..TYPES {
        out.push_str(&format!("type T{t} {{\n"));
        for f in 0..FIELDS {
            // 100 fields change their type, 100 are removed: 200 changes
            let n = t * FIELDS + f;
            if changed && n % 100 == 1 && n / 100 < 100 {
                out.push_str(&format!("  f{f}(a: Int, b: String): Int\n"));
            } else if changed && n % 100 == 2 && n / 100 < 100 {
                continue;
            } else {
                out.push_str(&format!("  f{f}(a: Int, b: String): String!\n"));
            }
        }
        out.push_str("}\n");
    }
    out
}

fn proto(changed: bool) -> String {
    let mut out = String::from("syntax = \"proto3\";\npackage perf;\n");
    for m in 0..500 {
        out.push_str(&format!("message M{m} {{\n"));
        for f in 1..=10 {
            let ty = if changed && f == 3 && m < 100 {
                "string"
            } else {
                "int32"
            };
            if changed && f == 5 && (100..200).contains(&m) {
                continue;
            }
            out.push_str(&format!("  {ty} f{f} = {f};\n"));
        }
        out.push_str("}\n");
    }
    out.push_str("service S {\n");
    for m in 0..500 {
        out.push_str(&format!("  rpc R{m}(M{m}) returns (M{m});\n"));
    }
    out.push_str("}\n");
    out
}

fn collection() -> FileSet {
    let mut files = FileSet::default();
    files.files.insert(
        "opencollection.yml".into(),
        b"opencollection: 1.0.0\ninfo:\n  name: Perf\n".to_vec(),
    );
    for i in 0..50 {
        files.files.insert(
            format!("q{i}.yml"),
            format!(
                "info:\n  name: Q{i}\n  type: graphql\n  seq: {i}\n\ngraphql:\n  method: POST\n  url: \"{{{{baseUrl}}}}/graphql\"\n  body:\n    query: |-\n      query {{\n        t0 {{\n          f0\n          f1\n        }}\n      }}\n    variables: |-\n      {{}}\n  auth: inherit\n"
            )
            .into_bytes(),
        );
    }
    files
}

fn one(name: &str, text: String) -> SchemaSources {
    let mut s = SchemaSources::new();
    s.insert(name.into(), text.into_bytes());
    s
}

fn median(
    kind: SchemaKind,
    old: &SchemaSources,
    new: &SchemaSources,
    files: &FileSet,
) -> (f64, usize) {
    let mut times = Vec::new();
    let mut count = 0;
    for _ in 0..RUNS {
        let started = web_time::Instant::now();
        let c = schema_changes(kind, Some(old), Some(new), files).unwrap();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        count = c.changes.len();
    }
    times.sort_by(f64::total_cmp);
    (times[RUNS / 2], count)
}

#[test]
fn schema_changes_of_2000_types_and_500_messages_are_measured() {
    let files = collection();
    let (g_old, g_new) = (
        one("schema.graphql", graphql(false)),
        one("schema.graphql", graphql(true)),
    );
    let (graphql_ms, graphql_changes) = median(SchemaKind::Graphql, &g_old, &g_new, &files);
    assert_eq!(
        graphql_changes, CHANGES,
        "100 types changed and 100 fields removed"
    );
    let (p_old, p_new) = (one("a.proto", proto(false)), one("a.proto", proto(true)));
    let (protobuf_ms, protobuf_changes) = median(SchemaKind::Protobuf, &p_old, &p_new, &files);
    assert_eq!(
        protobuf_changes, 200,
        "100 types changed and 100 fields removed"
    );
    let slower = graphql_ms.max(protobuf_ms);
    if let Ok(dir) = std::env::var("RT_PERF_OUT") {
        let json = format!(
            "{{\"schema_changes\":{slower:.3},\"graphql\":{graphql_ms:.3},\"protobuf\":{protobuf_ms:.3},\"runs\":{RUNS}}}"
        );
        std::fs::write(std::path::Path::new(&dir).join("schemas.json"), json).unwrap();
    }
    // Debug builds are slower; the budget is for the release build the script measures.
    if !cfg!(debug_assertions) {
        assert!(slower < LIMIT_MS, "schema_changes {slower:.1} ms");
    }
}
