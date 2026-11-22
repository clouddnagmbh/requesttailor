// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! rt-grpc in the browser (wave 8, strand B, point 9): proto files with imports compile in
//! WebAssembly, JSON <-> protobuf and templates match the native results
//! (`crates/rt-grpc/tests/descriptors.rs`), and the GraphQL SDL printer runs.
//!
//! Run: `RT_CONFORMANCE_PORT=47872 scripts/conformance-wasm.sh -- --test grpc_browser`

#![cfg(target_arch = "wasm32")]

use rt_app::{SendInput, Workspace, graphql, grpc};
use rt_format::FileSet;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const ROOT: &str = "opencollection: 1.0.0\ninfo:\n  name: g\nconfig:\n  protobuf:\n    protoFiles:\n      - path: protos/echo.proto\n    importPaths:\n      - path: include\n";
const REQ: &str = "info:\n  name: echo\n  type: grpc\n\ngrpc:\n  url: localhost:1\n  method: /rt.echo.v1.EchoService/Echo\n  methodType: unary\n";

fn descriptors() -> grpc::Descriptors {
    let mut files = FileSet::default();
    for (p, t) in [
        ("opencollection.yml", ROOT),
        ("echo.yml", REQ),
        (
            "protos/echo.proto",
            include_str!("../../../testdata/grpc/protos/echo.proto"),
        ),
        (
            "include/rt/common.proto",
            include_str!("../../../testdata/grpc/include/rt/common.proto"),
        ),
    ] {
        files.files.insert(p.into(), t.as_bytes().to_vec());
    }
    let ws = Workspace::open(files).unwrap();
    grpc::snapshot(&ws, "echo.yml", SendInput::default())
        .unwrap()
        .local_descriptors()
        .unwrap()
        .unwrap()
}

#[wasm_bindgen_test]
fn proto_files_compile_and_messages_round_trip() {
    let d = descriptors();
    let services = d.services();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].methods.len(), 5);
    let echo = "/rt.echo.v1.EchoService/Echo";
    let bytes = d
        .encode(
            echo,
            r#"{"message": "hi", "priority": "PRIORITY_HIGH", "tags": ["a"], "sentAt": "2026-01-02T03:04:05Z", "number": "7"}"#,
        )
        .unwrap();
    let back = d.decode_input(echo, &bytes).unwrap();
    assert!(back.contains("\"message\": \"hi\""), "{back}");
    assert!(
        back.contains("\"sentAt\": \"2026-01-02T03:04:05Z\""),
        "{back}"
    );
    assert!(back.contains("\"number\": \"7\""), "{back}");
    assert!(d.encode(echo, r#"{"nope": 1}"#).is_err());
    let template = d.template("/rt.echo.v1.EchoService/Countdown").unwrap();
    assert_eq!(
        template,
        "{\n  \"message\": \"\",\n  \"count\": 0,\n  \"delayMs\": 0\n}"
    );
}

#[wasm_bindgen_test]
fn graphql_sdl_prints() {
    let schema = serde_json::json!({
        "queryType": { "name": "Query" },
        "types": [{ "kind": "OBJECT", "name": "Query", "fields": [
            { "name": "hello", "args": [], "type": { "kind": "SCALAR", "name": "String" } }
        ] }]
    });
    assert_eq!(
        graphql::sdl(&schema).unwrap(),
        "type Query {\n  hello: String\n}\n"
    );
}
