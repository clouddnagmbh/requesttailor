// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 8, strand B, point 1): proto files with imports and import paths,
//! services, JSON ↔ protobuf, templates, errors. Pure Rust; the same checks run in the
//! browser through `rt-wasm` (`tests/grpc_browser.rs`).

use rt_core::model::{GrpcMethodType, GrpcSpec, ProtobufConfig};
use rt_core::stream::{Direction, Frame, StreamEvent};
use rt_grpc::{Descriptors, GrpcError, ProtoSource, ProtoSources, proto_source};

const ECHO: &str = include_str!("../../../testdata/grpc/protos/echo.proto");
const COMMON: &str = include_str!("../../../testdata/grpc/include/rt/common.proto");
const ECHO_METHOD: &str = "/rt.echo.v1.EchoService/Echo";

fn sources() -> ProtoSources {
    vec![
        ("protos/echo.proto".into(), ECHO.into()),
        ("include/rt/common.proto".into(), COMMON.into()),
    ]
}

fn echo() -> Descriptors {
    Descriptors::from_files(
        &sources(),
        &["protos/echo.proto".into()],
        &["include".into()],
    )
    .expect("echo.proto compiles")
}

#[test]
fn proto_files_with_imports_and_import_paths() {
    let services = echo().services();
    assert_eq!(services.len(), 1);
    let service = &services[0];
    assert_eq!(service.name, "rt.echo.v1.EchoService");
    let types: Vec<(&str, GrpcMethodType)> = service
        .methods
        .iter()
        .map(|m| (m.name.as_str(), m.method_type))
        .collect();
    assert_eq!(
        types,
        [
            ("Echo", GrpcMethodType::Unary),
            ("Countdown", GrpcMethodType::ServerStreaming),
            ("Collect", GrpcMethodType::ClientStreaming),
            ("Chat", GrpcMethodType::BidiStreaming),
            ("Fail", GrpcMethodType::Unary),
        ]
    );
    let m = &service.methods[0];
    assert_eq!(m.path, ECHO_METHOD);
    assert_eq!(m.input_type, "rt.echo.v1.EchoRequest");
    assert_eq!(m.output_type, "rt.echo.v1.EchoReply");
}

#[test]
fn imports_resolve_relative_to_import_paths_and_paths_normalize() {
    // `./` and `..` in the request's path, the import path written with a trailing slash.
    let d = Descriptors::from_files(
        &sources(),
        &["include/../protos/./echo.proto".into()],
        &["include/".into()],
    )
    .unwrap();
    assert_eq!(d.services().len(), 1);
    // Loading on demand gives the same.
    let all = sources();
    let loaded =
        Descriptors::from_loader(&["protos/echo.proto".into()], &["include".into()], &|p| {
            all.iter().find(|(k, _)| k == p).map(|(_, s)| s.clone())
        })
        .unwrap();
    assert_eq!(loaded.services(), d.services());
}

#[test]
fn a_missing_import_names_the_file_and_line() {
    let err = Descriptors::from_files(&sources(), &["protos/echo.proto".into()], &[]).unwrap_err();
    let GrpcError::Proto(message) = err else {
        panic!("{err:?}");
    };
    assert!(message.contains("protos/echo.proto:12"), "{message}");
    assert!(message.contains("rt/common.proto"), "{message}");
}

#[test]
fn a_syntax_error_names_the_file_and_line() {
    let broken = vec![(
        "bad.proto".to_owned(),
        "syntax = \"proto3\";\n\nmessage A {\n  string a = ;\n}\n".to_owned(),
    )];
    let err = Descriptors::from_files(&broken, &["bad.proto".into()], &[]).unwrap_err();
    let GrpcError::Proto(message) = err else {
        panic!("{err:?}");
    };
    assert!(message.starts_with("bad.proto:4"), "{message}");
    let missing = Descriptors::from_files(&broken, &["nope.proto".into()], &[]).unwrap_err();
    assert_eq!(
        missing,
        GrpcError::Proto("nope.proto: file not found".into())
    );
}

#[test]
fn json_encodes_to_protobuf_and_decodes_back() {
    let d = echo();
    let json = r#"{
        "message": "hi",
        "priority": "PRIORITY_HIGH",
        "tags": ["a", "b"],
        "labels": {"k": "v"},
        "sentAt": "2026-11-22T10:00:00Z",
        "number": "42"
    }"#;
    let bytes = d.encode(ECHO_METHOD, json).unwrap();
    assert!(!bytes.is_empty());
    let back: serde_json::Value =
        serde_json::from_str(&d.decode_input(ECHO_METHOD, &bytes).unwrap()).unwrap();
    assert_eq!(
        back,
        serde_json::json!({
            "message": "hi",
            "priority": "PRIORITY_HIGH",
            "tags": ["a", "b"],
            "labels": {"k": "v"},
            "sentAt": "2026-11-22T10:00:00Z",
            "number": "42"
        })
    );
    // Field 1 (`message`), length-delimited: the bytes are real protobuf.
    assert_eq!(&bytes[..4], &[0x0a, 0x02, b'h', b'i']);
    // The output type decodes as pretty JSON.
    let reply = d
        .decode(ECHO_METHOD, &[0x0a, 0x02, b'h', b'i', 0x10, 0x07])
        .unwrap();
    assert_eq!(reply, "{\n  \"message\": \"hi\",\n  \"index\": 7\n}");
    // Empty text is the empty message.
    assert_eq!(d.encode(ECHO_METHOD, "  ").unwrap(), Vec::<u8>::new());
}

#[test]
fn bad_json_is_a_message_error() {
    let d = echo();
    for bad in [
        r#"{"nope": 1}"#,
        r#"{"message": 5}"#,
        "{",
        r#"{"message": "a"} x"#,
    ] {
        let err = d.encode(ECHO_METHOD, bad).unwrap_err();
        assert!(matches!(err, GrpcError::Message(_)), "{bad}: {err:?}");
    }
    assert!(matches!(
        d.decode(ECHO_METHOD, &[0xff, 0xff]).unwrap_err(),
        GrpcError::Message(_)
    ));
    assert_eq!(
        d.encode("/rt.echo.v1.EchoService/Nope", "{}").unwrap_err(),
        GrpcError::UnknownMethod("/rt.echo.v1.EchoService/Nope".into())
    );
    assert!(matches!(
        d.template("garbage").unwrap_err(),
        GrpcError::UnknownMethod(_)
    ));
}

#[test]
fn template_lists_every_field_and_encodes() {
    let d = echo();
    let template = d.template(ECHO_METHOD).unwrap();
    let value: serde_json::Value = serde_json::from_str(&template).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "message": "",
            "priority": "PRIORITY_UNSPECIFIED",
            "tags": [""],
            "labels": {"key": ""},
            "sentAt": "1970-01-01T00:00:00Z",
            "number": "0"
        })
    );
    d.encode(ECHO_METHOD, &template)
        .expect("the template is a valid message");
    let countdown = d.template("/rt.echo.v1.EchoService/Countdown").unwrap();
    assert_eq!(
        countdown,
        "{\n  \"message\": \"\",\n  \"count\": 0,\n  \"delayMs\": 0\n}"
    );
}

#[test]
fn a_url_prefix_before_the_service_is_ignored() {
    let d = echo();
    assert_eq!(
        d.method("/api/v1/rt.echo.v1.EchoService/Echo")
            .unwrap()
            .path,
        ECHO_METHOD
    );
}

#[test]
fn events_decode_by_direction() {
    let d = echo();
    let sent = d.encode_frame(ECHO_METHOD, r#"{"message": "up"}"#).unwrap();
    let event = d.decode_event(
        ECHO_METHOD,
        StreamEvent::Message {
            at_ms: 1,
            direction: Direction::Sent,
            frame: sent,
            name: None,
            id: None,
        },
    );
    let StreamEvent::Message { frame, name, .. } = event else {
        panic!()
    };
    assert_eq!(frame, Frame::Text("{\n  \"message\": \"up\"\n}".into()));
    assert_eq!(name.as_deref(), Some("rt.echo.v1.EchoRequest"));
    let received = d.decode_event(
        ECHO_METHOD,
        StreamEvent::Message {
            at_ms: 2,
            direction: Direction::Received,
            frame: Frame::Binary(vec![0x10, 0x01]),
            name: None,
            id: None,
        },
    );
    let StreamEvent::Message { frame, name, .. } = received else {
        panic!()
    };
    assert_eq!(frame, Frame::Text("{\n  \"index\": 1\n}".into()));
    assert_eq!(name.as_deref(), Some("rt.echo.v1.EchoReply"));
    // Undecodable bytes stay binary; other events pass through.
    let junk = d.decode_event(
        ECHO_METHOD,
        StreamEvent::Message {
            at_ms: 3,
            direction: Direction::Received,
            frame: Frame::Binary(vec![0xff]),
            name: None,
            id: None,
        },
    );
    assert!(matches!(
        junk,
        StreamEvent::Message {
            frame: Frame::Binary(_),
            ..
        }
    ));
}

#[test]
fn proto_source_prefers_the_request_then_the_collection_then_reflection() {
    let mut spec = GrpcSpec {
        method: ECHO_METHOD.into(),
        proto_file: Some("../protos/echo.proto".into()),
        ..GrpcSpec::default()
    };
    let config = ProtobufConfig {
        proto_files: vec!["protos/./echo.proto".into()],
        import_paths: vec!["include".into()],
    };
    assert_eq!(
        proto_source(&config, "requests/echo.yml", &spec),
        ProtoSource::Files {
            files: vec!["protos/echo.proto".into()],
            import_paths: vec!["include".into()],
        }
    );
    spec.proto_file = None;
    assert_eq!(
        proto_source(&config, "requests/echo.yml", &spec),
        ProtoSource::Files {
            files: vec!["protos/echo.proto".into()],
            import_paths: vec!["include".into()],
        }
    );
    assert_eq!(
        proto_source(&ProtobufConfig::default(), "echo.yml", &spec),
        ProtoSource::Reflection
    );
}
