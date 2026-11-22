// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: grpc` items: `grpc.{url, method, methodType, protoFilePath, metadata, message, auth}` (message variants). Wave 8, strand B.
//!
//! Mapping:
//! - `url` → `RequestSpec.url`, `metadata` → `RequestSpec.headers` (same shape as HTTP
//!   headers), `auth` as for HTTP; `method` (`/package.Service/Method`), `methodType`,
//!   `protoFilePath` and the messages → [`GrpcSpec`]. `RequestSpec.method` is `GRPC`, there is
//!   no body and no query parameters.
//! - `message`: a string is one untitled variant; a list (`title`, `selected`, `message`)
//!   gives one variant each. Messages are JSON ([`MessageKind::Json`]).
//! - `runtime.{variables, scripts, assertions}`; gRPC script phases stay untouched in the file.
//! - OpenCollection has no `settings` and no `runtime.actions` for gRPC: edits that need them
//!   are refused.
//! - `config.protobuf` of `opencollection.yml` → [`ProtobufConfig`] ([`read_protobuf`]).

use rt_core::model::{
    Auth, Body, GrpcMethodType, GrpcSpec, MessageKind, MessageVariant, ProtobufConfig, Protocol,
    RequestEntry, RequestSpec,
};

use super::Result;
use crate::read::{self, Builder};
use crate::write::{self, Empty, ListSpec};
use crate::yaml::{Document, Map, Node, Val, document_text, reparse};

const GRPC_ORDER: &[&str] = &[
    "url",
    "method",
    "methodType",
    "protoFilePath",
    "metadata",
    "message",
    "auth",
];
const VARIANT_ORDER: &[&str] = &["title", "selected", "message"];

fn method_type(text: Option<&str>) -> Option<GrpcMethodType> {
    Some(match text {
        None | Some("unary") => GrpcMethodType::Unary,
        Some("client-streaming") => GrpcMethodType::ClientStreaming,
        Some("server-streaming") => GrpcMethodType::ServerStreaming,
        Some("bidi-streaming") => GrpcMethodType::BidiStreaming,
        Some(_) => return None,
    })
}

pub(crate) fn method_type_str(t: GrpcMethodType) -> &'static str {
    match t {
        GrpcMethodType::Unary => "unary",
        GrpcMethodType::ClientStreaming => "client-streaming",
        GrpcMethodType::ServerStreaming => "server-streaming",
        GrpcMethodType::BidiStreaming => "bidi-streaming",
    }
}

fn variant(node: &Node) -> MessageVariant {
    MessageVariant {
        title: read::text(node.get("title")),
        selected: read::flag(node.get("selected")) == Some(true),
        kind: MessageKind::Json,
        data: read::text(node.get("message")),
    }
}

fn messages(node: Option<&Node>) -> Vec<MessageVariant> {
    match node.filter(|n| !n.is_null()) {
        None => Vec::new(),
        Some(list) if list.is_seq() => list.items().map(variant).collect(),
        Some(single) => vec![MessageVariant {
            title: String::new(),
            selected: false,
            kind: MessageKind::Json,
            data: read::text(Some(single)),
        }],
    }
}

pub(crate) fn read(builder: &mut Builder<'_>, path: &str) -> Option<RequestEntry> {
    let root = &builder.docs.get(path)?.root;
    let info = root.get("info");
    let grpc = root.get("grpc");
    let field = |k: &str| grpc.and_then(|g| g.get(k));
    let kind_text = field("methodType").and_then(Node::text);
    let method_type = match method_type(kind_text) {
        Some(t) => t,
        None => {
            builder.issues.push(crate::FormatError::Invalid {
                path: path.to_owned(),
                message: format!("unknown gRPC methodType `{}`", kind_text.unwrap_or("")),
            });
            GrpcMethodType::Unary
        }
    };
    let runtime = root.get("runtime");
    let spec = RequestSpec {
        name: info
            .and_then(|i| i.get("name"))
            .and_then(Node::text)
            .unwrap_or_else(|| read::file_stem(path))
            .to_owned(),
        seq: info.and_then(|i| i.get("seq")).and_then(Node::as_f64),
        method: "GRPC".into(),
        url: read::text(field("url")),
        headers: read::key_values(field("metadata")),
        params: Vec::new(),
        body: Body::None,
        auth: Auth::None,
        vars: read::variables(runtime.and_then(|r| r.get("variables"))),
        settings: read::settings(root.get("settings")),
        scripts: crate::runtime::scripts(runtime.and_then(|r| r.get("scripts"))),
        assertions: crate::runtime::assertions(runtime.and_then(|r| r.get("assertions"))),
        actions: crate::runtime::actions(runtime.and_then(|r| r.get("actions"))),
        protocol: Protocol::Grpc(Box::new(GrpcSpec {
            method: read::text(field("method")),
            method_type,
            proto_file: field("protoFilePath")
                .and_then(Node::text)
                .map(str::to_owned),
            messages: messages(field("message")),
        })),
    };
    let auth = grpc.and_then(|g| g.get("auth"));
    let spec = RequestSpec {
        auth: builder.auth(path, auth, Auth::None),
        ..spec
    };
    Some(RequestEntry {
        path: path.to_owned(),
        spec,
    })
}

/// `config.protobuf`: proto file paths and the enabled import paths.
pub(crate) fn read_protobuf(node: Option<&Node>) -> ProtobufConfig {
    let Some(node) = node else {
        return ProtobufConfig::default();
    };
    ProtobufConfig {
        proto_files: node
            .get("protoFiles")
            .map(|l| {
                l.items()
                    .filter_map(|f| f.get("path").and_then(Node::text).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        import_paths: node
            .get("importPaths")
            .map(|l| {
                l.items()
                    .filter(|p| read::enabled(p))
                    .filter_map(|p| p.get("path").and_then(Node::text).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn grpc_spec(spec: &RequestSpec) -> Result<&GrpcSpec> {
    match &spec.protocol {
        Protocol::Grpc(g) => Ok(g),
        _ => Err("not a gRPC request".into()),
    }
}

fn message_val(messages: &[MessageVariant]) -> Option<Val> {
    match messages {
        [] => None,
        [one] if one.title.is_empty() && !one.selected => Some(Val::str(&one.data)),
        many => Some(Val::Seq(many.iter().map(variant_val).collect())),
    }
}

fn variant_val(v: &MessageVariant) -> Val {
    let mut e = vec![("title", Val::str(&v.title))];
    if v.selected {
        e.push(("selected", Val::bool(true)));
    }
    e.push(("message", Val::str(&v.data)));
    Val::Map(e)
}

fn edit_variant(m: &mut Map, old: &MessageVariant, new: &MessageVariant, nl: &str) {
    if old.title != new.title {
        m.put("title", &Val::str(&new.title), VARIANT_ORDER, false, nl);
    }
    if old.selected != new.selected {
        if new.selected {
            m.put("selected", &Val::bool(true), VARIANT_ORDER, false, nl);
        } else {
            m.remove("selected");
        }
    }
    if old.data != new.data {
        m.put("message", &Val::str(&new.data), VARIANT_ORDER, false, nl);
    }
}

fn grpc_val(spec: &RequestSpec, g: &GrpcSpec) -> Result<Val> {
    let mut e = vec![
        ("url", Val::str(&spec.url)),
        ("method", Val::str(&g.method)),
        ("methodType", Val::str(method_type_str(g.method_type))),
    ];
    if let Some(p) = &g.proto_file {
        e.push(("protoFilePath", Val::str(p)));
    }
    if !spec.headers.is_empty() {
        e.push((
            "metadata",
            Val::Seq(spec.headers.iter().map(write::kv_val).collect()),
        ));
    }
    if let Some(m) = message_val(&g.messages) {
        e.push(("message", m));
    }
    if let Some(auth) = write::auth_val(&spec.auth)? {
        e.push(("auth", auth));
    }
    Ok(Val::Map(e))
}

/// Text of a new gRPC request file, as Bruno writes one.
pub(crate) fn new_text(spec: &RequestSpec) -> String {
    let mut info = vec![("name", Val::str(&spec.name)), ("type", Val::str("grpc"))];
    if let Some(seq) = spec.seq {
        info.push(("seq", Val::Plain(write::number(seq))));
    }
    let g = grpc_spec(spec).cloned().unwrap_or_default();
    let mut grpc = vec![
        ("url", Val::str(&spec.url)),
        ("method", Val::str(&g.method)),
        ("methodType", Val::str(method_type_str(g.method_type))),
    ];
    if let Some(p) = &g.proto_file {
        grpc.push(("protoFilePath", Val::str(p)));
    }
    document_text(&[("info", Val::Map(info)), ("grpc", Val::Map(grpc))], true)
}

fn read_spec(doc: &Document, path: &str) -> Option<RequestSpec> {
    let docs = std::collections::BTreeMap::from([(path, doc)]);
    let mut builder = Builder::new(&docs);
    read(&mut builder, path).map(|r| r.spec)
}

pub(crate) fn apply(doc: &Document, path: &str, spec: &RequestSpec) -> Result<Document> {
    let g = grpc_spec(spec)?;
    let old = read_spec(doc, path).ok_or("not a gRPC request")?;
    if old == *spec {
        return Ok(doc.clone());
    }
    if spec.body != Body::None || !spec.params.is_empty() {
        return Err("gRPC requests have no body and no query parameters".into());
    }
    if spec.method != old.method {
        return Err("the method of a gRPC request is always GRPC".into());
    }
    if spec.settings != old.settings {
        return Err("OpenCollection has no settings for gRPC requests".into());
    }
    if spec.actions != old.actions {
        return Err("OpenCollection has no runtime actions for gRPC requests".into());
    }
    let old_g = grpc_spec(&old)?.clone();
    let mut doc = doc.clone();
    let nl = doc.newline;
    let root = doc
        .root
        .block_map_mut()
        .ok_or("the file is not a YAML mapping")?;
    write::write_info(root, &old, spec, nl);
    match root.get_mut("grpc").and_then(Node::block_map_mut) {
        None => root.put("grpc", &grpc_val(spec, g)?, write::ROOT_ORDER, true, nl),
        Some(grpc) => write_grpc(grpc, &old, &old_g, spec, g, nl)?,
    }
    write::write_runtime(root, &old, spec, nl)?;

    let doc = reparse(&doc)?;
    match read_spec(&doc, path) {
        Some(got) if got == *spec => Ok(doc),
        got => Err(format!(
            "internal error: the edited file does not read back as the request \
             (wanted {spec:?}, got {got:?})"
        )),
    }
}

fn write_grpc(
    grpc: &mut Map,
    old: &RequestSpec,
    old_g: &GrpcSpec,
    new: &RequestSpec,
    g: &GrpcSpec,
    nl: &str,
) -> Result<()> {
    if old.url != new.url {
        grpc.put("url", &Val::str(&new.url), GRPC_ORDER, false, nl);
    }
    if old_g.method != g.method {
        grpc.put("method", &Val::str(&g.method), GRPC_ORDER, false, nl);
    }
    if old_g.method_type != g.method_type {
        grpc.put(
            "methodType",
            &Val::str(method_type_str(g.method_type)),
            GRPC_ORDER,
            false,
            nl,
        );
    }
    if old_g.proto_file != g.proto_file {
        match &g.proto_file {
            Some(p) => grpc.put("protoFilePath", &Val::str(p), GRPC_ORDER, false, nl),
            None => {
                grpc.remove("protoFilePath");
            }
        }
    }
    let metadata = ListSpec {
        key: "metadata",
        order: GRPC_ORDER,
        ..write::header_list()
    };
    write::sync_list(grpc, &metadata, &new.headers, nl);
    if old_g.messages != g.messages {
        write_messages(grpc, &g.messages, nl);
    }
    if old.auth != new.auth {
        if grpc.index("auth").is_none() {
            if let Some(val) = write::auth_val(&new.auth)? {
                grpc.put("auth", &val, GRPC_ORDER, false, nl);
            }
        } else {
            write::write_auth(grpc, &old.auth, &new.auth, nl)?;
        }
    }
    Ok(())
}

fn write_messages(grpc: &mut Map, messages: &[MessageVariant], nl: &str) {
    let is_list = grpc.get("message").is_some_and(Node::is_seq);
    match messages {
        [] => {
            grpc.remove("message");
        }
        // One untitled message stays (or becomes) a plain string unless the file has a list.
        [one] if one.title.is_empty() && !one.selected && !is_list => {
            grpc.put("message", &Val::str(&one.data), GRPC_ORDER, false, nl);
        }
        _ if is_list => write::sync_list(
            grpc,
            &ListSpec {
                key: "message",
                order: GRPC_ORDER,
                empty: Empty::Remove,
                read: &variant,
                same: &|a: &MessageVariant, b: &MessageVariant| a.title == b.title,
                render: &variant_val,
                edit: &edit_variant,
            },
            messages,
            nl,
        ),
        many => grpc.put(
            "message",
            &Val::Seq(many.iter().map(variant_val).collect()),
            GRPC_ORDER,
            false,
            nl,
        ),
    }
}
