// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Protocols besides plain HTTP (wave 8).
//!
//! A request file says what it speaks with OpenCollection's `info.type`: `http` (also SSE,
//! which is HTTP with a `text/event-stream` response), `graphql` (HTTP with a
//! [`Body::GraphQl`](super::Body::GraphQl) body), `grpc` and `websocket`. The last two are
//! sessions rather than one request and one response; they keep the rest of
//! [`RequestSpec`](super::RequestSpec) (url, headers as gRPC metadata or WebSocket handshake
//! headers, auth, vars, scripts) and add what only they have here.
//!
//! Owner: wave 8 interface. Format: strand B; sessions: strand A (WebSocket, SSE) and B (gRPC).

use serde::{Deserialize, Serialize};

/// What a request speaks. `Http` covers REST, SSE and GraphQL.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Protocol {
    #[default]
    Http,
    Grpc(Box<GrpcSpec>),
    #[serde(rename = "websocket")]
    WebSocket(Box<WebSocketSpec>),
}

impl Protocol {
    pub fn is_http(&self) -> bool {
        matches!(self, Protocol::Http)
    }
}

/// OpenCollection `grpc`. The URL (`grpc://`, `grpcs://` or `host:port`) is
/// `RequestSpec.url`, metadata is `RequestSpec.headers`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GrpcSpec {
    /// Full method name, `/package.Service/Method` (the leading `/` as in the file).
    pub method: String,
    pub method_type: GrpcMethodType,
    /// Proto file, relative to the request file as in OpenCollection `protoFilePath`;
    /// `None`: server reflection or the collection's `config.protobuf`.
    pub proto_file: Option<String>,
    /// Message variants as JSON text (proto3 JSON mapping).
    pub messages: Vec<MessageVariant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrpcMethodType {
    #[default]
    Unary,
    ClientStreaming,
    ServerStreaming,
    BidiStreaming,
}

/// OpenCollection `websocket`. URL `ws://`/`wss://` is `RequestSpec.url`, handshake headers
/// are `RequestSpec.headers`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WebSocketSpec {
    pub messages: Vec<MessageVariant>,
    /// `settings.timeout`: connect timeout in ms; `None` inherits, `Some(0)` none.
    pub timeout_ms: Option<u64>,
    /// `settings.keepAliveInterval` in ms; `None` inherits, `Some(0)` off.
    pub keep_alive_ms: Option<u64>,
}

/// One saved message (OpenCollection `message`, a single one or titled variants). A file
/// with a single untitled message has one variant with an empty title.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MessageVariant {
    pub title: String,
    pub selected: bool,
    pub kind: MessageKind,
    /// Text as written; `{{variables}}` are resolved when sent. Binary: base64.
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    #[default]
    Text,
    Json,
    Xml,
    Binary,
}

/// OpenCollection `config.protobuf` of `opencollection.yml`: proto files and import paths,
/// relative to the collection root.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProtobufConfig {
    pub proto_files: Vec<String>,
    /// Enabled import paths only (disabled ones stay in the file).
    pub import_paths: Vec<String>,
}
