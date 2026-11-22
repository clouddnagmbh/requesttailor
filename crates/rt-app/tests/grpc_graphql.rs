// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 8, strand B, points 5 and 6), natively against `rt-testserver`: the
//! GraphQL schema by introspection, sent like the request (auth reaches the server), as SDL;
//! gRPC services and templates by server reflection.

#![cfg(not(target_arch = "wasm32"))]

use std::net::SocketAddr;

use rt_app::{SendInput, Workspace, graphql, grpc};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use rt_format::FileSet;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn workspace(list: &[(&str, &str)]) -> Workspace {
    let mut files = FileSet::default();
    files.files.insert(
        "opencollection.yml".into(),
        b"opencollection: 1.0.0\ninfo:\n  name: t\n".to_vec(),
    );
    for (p, t) in list {
        files.files.insert((*p).into(), t.as_bytes().to_vec());
    }
    Workspace::open(files).unwrap()
}

#[test]
fn graphql_schema_by_introspection_as_sdl() {
    let rt = runtime();
    rt.block_on(async {
        let addr = rt_testserver::spawn_local().await;
        let ws = workspace(&[(
            "q.yml",
            "info:\n  name: q\n  type: graphql\n\ngraphql:\n  method: POST\n  url: \"{{api}}/graphql\"\n  body:\n    query: \"{ hello }\"\n    variables: \"\"\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n",
        )]);
        let input = SendInput {
            vars: vec![
                ("api".into(), format!("http://{addr}")),
                ("token".into(), "s3cret".into()),
            ],
            ..SendInput::default()
        };
        let transport = NativeTransport::new().unwrap();
        // The request itself, with its auth.
        let report = ws
            .snapshot("q.yml", input.clone())
            .unwrap()
            .send(&transport, &SendOptions::default())
            .await;
        let rt_core::send::ReportBody::Utf8(body) = &report.response.as_ref().unwrap().body else {
            panic!()
        };
        assert_eq!(body, r#"{"data":{"hello":"Hello, world (Bearer s3cret)!"}}"#);

        let snap = graphql::introspection_snapshot(&ws, "q.yml", input.clone()).unwrap();
        let report = snap.send(&transport, &SendOptions::default()).await;
        let sdl = graphql::schema_from_report(&report).unwrap();
        assert!(sdl.contains("type Query {\n"), "{sdl}");
        assert!(sdl.contains("  addBook(input: BookInput!): Book!\n"), "{sdl}");
        assert!(sdl.contains("enum Genre {\n  FICTION\n  SCIENCE\n}\n"), "{sdl}");
        assert_eq!(
            graphql::cache_key(&ws, "q.yml", &input).unwrap(),
            format!("\nhttp://{addr}/graphql")
        );

        // Not a GraphQL endpoint: a readable error.
        let ws = workspace(&[(
            "e.yml",
            "info:\n  name: e\n  type: http\n\nhttp:\n  method: GET\n  url: \"{{api}}/status/404\"\n",
        )]);
        let snap = graphql::introspection_snapshot(&ws, "e.yml", input).unwrap();
        let err = graphql::schema_from_report(&snap.send(&transport, &SendOptions::default()).await)
            .unwrap_err();
        assert!(err.to_string().contains("GraphQL introspection failed"), "{err}");
    });
}

#[test]
fn grpc_services_and_templates_by_reflection() {
    let rt = runtime();
    rt.block_on(async {
        let addr = rt_testserver::spawn_grpc(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let ws = workspace(&[(
            "echo.yml",
            "info:\n  name: echo\n  type: grpc\n\ngrpc:\n  url: \"grpc://{{host}}\"\n  method: /rt.echo.v1.EchoService/Echo\n  methodType: unary\n",
        )]);
        let input = SendInput {
            vars: vec![("host".into(), addr.to_string())],
            ..SendInput::default()
        };
        let snap = grpc::snapshot(&ws, "echo.yml", input).unwrap();
        let transport = NativeTransport::new().unwrap();
        let services = grpc::services(&snap, &transport).await.unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "rt.echo.v1.EchoService");
        assert_eq!(services[0].methods.len(), 5);
        let template = grpc::template(&snap, "/rt.echo.v1.EchoService/Countdown", &transport)
            .await
            .unwrap();
        assert_eq!(
            template,
            "{\n  \"message\": \"\",\n  \"count\": 0,\n  \"delayMs\": 0\n}"
        );
    });
}
