// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 8, strand B, point 4): gRPC items (`type: grpc`) and `config.protobuf`.
//! Reading the Bruno corpus, byte-identical round trip, edits with the read-back rule, new
//! files.

use std::path::{Path, PathBuf};

use rt_core::model::{
    Auth, GrpcMethodType, GrpcSpec, Item, KeyValue, MessageKind, MessageVariant, ProtobufConfig,
    Protocol, RequestSpec, ScriptPhase, Settings, Variable,
};
use rt_format::{CollectionDoc, FileSet, read_collection};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno")
}

fn corpus(name: &str) -> CollectionDoc {
    read_collection(&FileSet::from_dir(&corpus_dir().join(name)).unwrap()).unwrap()
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

fn grpc(spec: &RequestSpec) -> &GrpcSpec {
    match &spec.protocol {
        Protocol::Grpc(g) => g,
        other => panic!("not gRPC: {other:?}"),
    }
}

fn grpc_mut(spec: &mut RequestSpec) -> &mut GrpcSpec {
    match &mut spec.protocol {
        Protocol::Grpc(g) => g,
        other => panic!("not gRPC: {other:?}"),
    }
}

fn paths(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Request(r) => out.push(r.path.clone()),
            Item::Folder(f) => paths(&f.items, out),
        }
    }
}

fn all_roots() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if dir.join("opencollection.yml").is_file() {
            out.push(dir.to_path_buf());
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&corpus_dir(), &mut out);
    out
}

fn changed(a: &FileSet, b: &FileSet) -> Vec<String> {
    let mut out: Vec<String> = a
        .files
        .iter()
        .filter(|(p, bytes)| b.files.get(*p) != Some(*bytes))
        .map(|(p, _)| p.clone())
        .collect();
    out.extend(
        b.files
            .keys()
            .filter(|p| !a.files.contains_key(*p))
            .cloned(),
    );
    out
}

/// Every gRPC request of the corpus: (collection root, request path).
fn corpus_grpc() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for root in all_roots() {
        let doc = read_collection(&FileSet::from_dir(&root).unwrap()).unwrap();
        let mut list = Vec::new();
        paths(&doc.model().items, &mut list);
        for path in list {
            if matches!(spec(&doc, &path).protocol, Protocol::Grpc(_)) {
                out.push((root.clone(), path));
            }
        }
    }
    out
}

#[test]
fn the_corpus_has_28_grpc_requests() {
    assert_eq!(corpus_grpc().len(), 28);
}

#[test]
fn grpc_items_read_into_the_model() {
    let doc = corpus("grpc-run-lifecycle-hooks");
    let s = spec(&doc, "BidiHello.yml");
    assert_eq!(s.name, "BidiHello");
    assert_eq!(s.seq, Some(7.0));
    assert_eq!(s.method, "GRPC");
    assert_eq!(s.url, "{{host}}");
    let g = grpc(&s);
    assert_eq!(g.method, "/hello.HelloService/BidiHello");
    assert_eq!(g.method_type, GrpcMethodType::BidiStreaming);
    assert_eq!(g.proto_file, None);
    let titles: Vec<&str> = g.messages.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["message 1", "message 2", "message 3"]);
    assert_eq!(g.messages[0].kind, MessageKind::Json);
    assert_eq!(g.messages[0].data, "{\n  \"greeting\": \"bidi-1\"\n}");
    // gRPC script phases are not request scripts; they stay in the file untouched.
    assert!(
        s.scripts
            .iter()
            .all(|sc| sc.phase != ScriptPhase::BeforeRequest)
    );

    let doc = corpus("grpc-subpath");
    let s = spec(&doc, "ProtoFileUnderSubpath.yml");
    assert_eq!(s.url, "grpc://localhost:50051/Hello.Api");
    assert_eq!(grpc(&s).method_type, GrpcMethodType::Unary);
    assert_eq!(
        grpc(&s).proto_file.as_deref(),
        Some("../../../../../../packages/bruno-tests/src/grpc/hello.proto")
    );
    assert_eq!(
        doc.model().config.protobuf,
        ProtobufConfig {
            proto_files: vec!["../../../../../../packages/bruno-tests/src/grpc/hello.proto".into()],
            import_paths: Vec::new(),
        }
    );

    let s = spec(&corpus("collection-export"), "grpc-request.yml");
    assert_eq!(grpc(&s).method, "/hello.HelloService/SayHello");
    assert!(grpc(&s).messages.is_empty());
    assert_eq!(s.auth, Auth::None);
}

const SINGLE: &str = "info:
  name: echo
  type: grpc
  seq: 1

grpc:
  url: grpc://localhost:47885
  method: /rt.echo.v1.EchoService/Echo
  methodType: unary
  protoFilePath: protos/echo.proto
  metadata:
    - name: x-trace
      value: \"{{trace}}\"
      description: kept
    - name: x-off
      value: \"1\"
      disabled: true
  message: |-
    {
      \"message\": \"hi\"
    }
  auth:
    type: bearer
    token: \"{{token}}\"

runtime:
  variables:
    - name: trace
      value: t-1
";

fn single() -> FileSet {
    files(&[
        (
            "opencollection.yml",
            "opencollection: 1.0.0\ninfo:\n  name: g\nconfig:\n  protobuf:\n    protoFiles:\n      - path: protos/echo.proto\n        type: file\n    importPaths:\n      - path: include\n      - path: old\n        disabled: true\n",
        ),
        ("echo.yml", SINGLE),
    ])
}

#[test]
fn metadata_auth_vars_and_protobuf_config() {
    let doc = read_collection(&single()).unwrap();
    assert!(doc.issues().is_empty(), "{:?}", doc.issues());
    let s = spec(&doc, "echo.yml");
    assert_eq!(
        s.headers,
        [
            KeyValue::new("x-trace", "{{trace}}"),
            KeyValue {
                enabled: false,
                ..KeyValue::new("x-off", "1")
            }
        ]
    );
    assert_eq!(
        s.auth,
        Auth::Bearer {
            token: "{{token}}".into()
        }
    );
    assert_eq!(s.vars, [Variable::new("trace", "t-1")]);
    assert_eq!(
        grpc(&s).messages,
        [MessageVariant {
            title: String::new(),
            selected: false,
            kind: MessageKind::Json,
            data: "{\n  \"message\": \"hi\"\n}".into(),
        }]
    );
    assert_eq!(
        doc.model().config.protobuf,
        ProtobufConfig {
            proto_files: vec!["protos/echo.proto".into()],
            import_paths: vec!["include".into()],
        }
    );
}

#[test]
fn edits_touch_only_what_changed() {
    let input = single();
    let mut doc = read_collection(&input).unwrap();
    let mut s = spec(&doc, "echo.yml");
    grpc_mut(&mut s).messages[0].data = "{\"message\": \"bye\"}".into();
    s.url = "grpcs://api.example".into();
    s.headers[0].value = "t-2".into();
    doc.set_request("echo.yml", &s).unwrap();
    let written = text(&doc.write(), "echo.yml");
    let expected = SINGLE
        .replace("grpc://localhost:47885", "grpcs://api.example")
        .replace("value: \"{{trace}}\"", "value: t-2")
        .replace(
            "  message: |-\n    {\n      \"message\": \"hi\"\n    }\n",
            "  message: '{\"message\": \"bye\"}'\n",
        );
    assert_eq!(written, expected);
    assert_eq!(spec(&read_collection(&doc.write()).unwrap(), "echo.yml"), s);
}

#[test]
fn messages_become_variants_and_back() {
    let mut doc = read_collection(&single()).unwrap();
    let before = spec(&doc, "echo.yml");
    let mut s = before.clone();
    let g = grpc_mut(&mut s);
    g.messages = vec![
        MessageVariant {
            title: "first".into(),
            selected: false,
            kind: MessageKind::Json,
            data: "{}".into(),
        },
        MessageVariant {
            title: "second".into(),
            selected: true,
            kind: MessageKind::Json,
            data: "{\n  \"message\": \"two\"\n}".into(),
        },
    ];
    g.method_type = GrpcMethodType::BidiStreaming;
    g.method = "/rt.echo.v1.EchoService/Chat".into();
    g.proto_file = None;
    doc.set_request("echo.yml", &s).unwrap();
    let written = text(&doc.write(), "echo.yml");
    assert!(
        written.contains("  methodType: bidi-streaming\n  metadata:"),
        "{written}"
    );
    assert!(!written.contains("protoFilePath"), "{written}");
    assert!(
        written.contains(
            "  message:\n    - title: first\n      message: \"{}\"\n    - title: second\n      selected: true\n      message: |-\n        {\n          \"message\": \"two\"\n        }\n"
        ),
        "{written}"
    );
    assert_eq!(spec(&read_collection(&doc.write()).unwrap(), "echo.yml"), s);
    // Edit one variant in the list: the other stays.
    let mut s2 = s.clone();
    grpc_mut(&mut s2).messages[0].data = "{\"a\": 1}".into();
    doc.set_request("echo.yml", &s2).unwrap();
    let written2 = text(&doc.write(), "echo.yml");
    assert_eq!(
        written2,
        written.replace("message: \"{}\"", "message: '{\"a\": 1}'")
    );
    // And back to the original file.
    doc.set_request("echo.yml", &before).unwrap();
    assert_eq!(spec(&doc, "echo.yml"), before);
    assert_eq!(
        spec(&read_collection(&doc.write()).unwrap(), "echo.yml"),
        before
    );
}

#[test]
fn what_opencollection_cannot_hold_for_grpc_is_refused() {
    let mut doc = read_collection(&single()).unwrap();
    let before = doc.write();
    let s = spec(&doc, "echo.yml");
    let mut with_settings = s.clone();
    with_settings.settings = Settings {
        timeout_ms: Some(5),
        ..Settings::default()
    };
    assert!(doc.set_request("echo.yml", &with_settings).is_err());
    let mut with_method = s.clone();
    with_method.method = "POST".into();
    assert!(doc.set_request("echo.yml", &with_method).is_err());
    assert_eq!(changed(&before, &doc.write()), Vec::<String>::new());
}

#[test]
fn every_corpus_grpc_request_can_be_edited_and_restored() {
    for (root, path) in corpus_grpc() {
        let ctx = format!("{}: {path}", root.display());
        let input = FileSet::from_dir(&root).unwrap();
        let mut doc = read_collection(&input).unwrap();
        let original = doc.write();
        let before = spec(&doc, &path);
        let mut after = before.clone();
        after.name = format!("{} (edited)", before.name);
        after.url = "grpc://edited:1".into();
        after.headers.insert(0, KeyValue::new("x-added", "yes"));
        after.auth = Auth::Basic {
            username: "u".into(),
            password: "{{pw}}".into(),
        };
        after.vars.push(Variable::new("added", "1"));
        let g = grpc_mut(&mut after);
        g.method = format!("{}2", g.method);
        g.method_type = GrpcMethodType::ClientStreaming;
        g.proto_file = Some("edited.proto".into());
        g.messages.push(MessageVariant {
            title: "added".into(),
            selected: true,
            kind: MessageKind::Json,
            data: "{\"x\": 1}".into(),
        });
        doc.set_request(&path, &after)
            .unwrap_or_else(|e| panic!("{ctx}: {e}"));
        let written = doc.write();
        assert_eq!(changed(&original, &written), vec![path.clone()], "{ctx}");
        assert_eq!(
            spec(&read_collection(&written).unwrap(), &path),
            after,
            "{ctx}"
        );
        doc.set_request(&path, &before)
            .unwrap_or_else(|e| panic!("{ctx}: back: {e}"));
        assert_eq!(
            spec(&read_collection(&doc.write()).unwrap(), &path),
            before,
            "{ctx}: back"
        );
    }
}

#[test]
fn new_grpc_requests_are_laid_out_like_bruno() {
    let mut doc = read_collection(&files(&[(
        "opencollection.yml",
        "opencollection: 1.0.0\ninfo:\n  name: g\n",
    )]))
    .unwrap();
    let s = RequestSpec {
        name: "Say Hello".into(),
        method: "GRPC".into(),
        url: "grpc://localhost:50051".into(),
        headers: vec![KeyValue::new("x-a", "1")],
        protocol: Protocol::Grpc(Box::new(GrpcSpec {
            method: "/hello.HelloService/SayHello".into(),
            method_type: GrpcMethodType::Unary,
            proto_file: None,
            messages: vec![MessageVariant {
                title: String::new(),
                selected: false,
                kind: MessageKind::Json,
                data: "{\n  \"greeting\": \"hi\"\n}".into(),
            }],
        })),
        ..RequestSpec::default()
    };
    let path = doc.add_request("", &s).unwrap();
    assert_eq!(
        text(&doc.write(), &path),
        "info:\n  name: Say Hello\n  type: grpc\n  seq: 1\n\ngrpc:\n  url: grpc://localhost:50051\n  method: /hello.HelloService/SayHello\n  methodType: unary\n  metadata:\n    - name: x-a\n      value: \"1\"\n  message: |-\n    {\n      \"greeting\": \"hi\"\n    }\n  auth: inherit\n"
    );
    let mut want = s.clone();
    want.seq = Some(1.0);
    assert_eq!(spec(&read_collection(&doc.write()).unwrap(), &path), want);
}
