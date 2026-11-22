// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-grpc` (wave 8, strand B): what a gRPC request needs above the wire.
//!
//! - [`Descriptors`] from proto files ([`Descriptors::from_files`], collection
//!   `config.protobuf` or the request's `protoFilePath`) or from server reflection
//!   ([`Descriptors::from_reflection`], `grpc.reflection.v1` with a fallback to `v1alpha`,
//!   over a [`StreamTransport`]).
//! - Messages are written and shown as JSON (proto3 JSON mapping): [`Descriptors::encode`] and
//!   [`Descriptors::decode`] convert for one method; [`Descriptors::template`] gives a JSON
//!   skeleton of the input message for the editor.
//! - [`rt_core::stream`] moves the bytes (native HTTP/2, or the companion in the browser).
//! - [`proto_source`] says where a request's descriptors come from; [`Descriptors::encode_frame`]
//!   and [`Descriptors::decode_event`] connect messages to a session.
//!
//! Proto files are compiled in pure Rust (`protox`), messages are dynamic (`prost-reflect`).
//! Builds for every target, wasm32 and iOS included.

mod files;
mod json;
mod reflection;

use prost_reflect::{DescriptorPool, MethodDescriptor};
use rt_core::model::{GrpcMethodType, GrpcSpec, ProtobufConfig};
use rt_core::stream::{Frame, StreamEvent, StreamTransport};
use serde::{Deserialize, Serialize};

pub use files::{Loader, join as join_path, normalize as normalize_path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrpcService {
    /// Fully qualified, e.g. `hello.HelloService`.
    pub name: String,
    pub methods: Vec<GrpcMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrpcMethod {
    pub name: String,
    /// `/hello.HelloService/SayHello`, as in the request file.
    pub path: String,
    pub method_type: GrpcMethodType,
    pub input_type: String,
    pub output_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum GrpcError {
    /// A proto file is missing or does not compile (file, line and message).
    #[error("proto: {0}")]
    Proto(String),
    /// The server has no reflection service, or it failed.
    #[error("server reflection: {0}")]
    Reflection(String),
    #[error("unknown method {0}")]
    UnknownMethod(String),
    /// The JSON does not match the message type, or the bytes do not decode.
    #[error("message: {0}")]
    Message(String),
}

/// Compiled descriptors of one or more proto files (or of what reflection returned).
#[derive(Debug, Clone, Default)]
pub struct Descriptors {
    pool: DescriptorPool,
}

/// Proto sources by path relative to the collection root (`/`-separated); the caller reads
/// them (desktop and CLI from disk, web from the opened folder).
pub type ProtoSources = Vec<(String, String)>;

/// Where the descriptors of a gRPC request come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProtoSource {
    /// Proto files and import paths, relative to the collection root.
    Files {
        files: Vec<String>,
        import_paths: Vec<String>,
    },
    /// Ask the server.
    Reflection,
}

/// The request's `protoFilePath` (relative to the request file) if it has one, else the
/// collection's `config.protobuf`, else server reflection. `request_path` is relative to the
/// collection root.
pub fn proto_source(config: &ProtobufConfig, request_path: &str, spec: &GrpcSpec) -> ProtoSource {
    let import_paths = config
        .import_paths
        .iter()
        .map(|p| files::normalize(p))
        .collect();
    if let Some(file) = spec.proto_file.as_deref().filter(|f| !f.trim().is_empty()) {
        let dir = request_path.rsplit_once('/').map_or("", |(d, _)| d);
        return ProtoSource::Files {
            files: vec![files::join(dir, file)],
            import_paths,
        };
    }
    if !config.proto_files.is_empty() {
        return ProtoSource::Files {
            files: config
                .proto_files
                .iter()
                .map(|p| files::normalize(p))
                .collect(),
            import_paths,
        };
    }
    ProtoSource::Reflection
}

impl Descriptors {
    /// Compiles `files` (paths into `sources`) with `import_paths`.
    pub fn from_files(
        sources: &ProtoSources,
        files: &[String],
        import_paths: &[String],
    ) -> Result<Self, GrpcError> {
        let by_path: std::collections::BTreeMap<String, &str> = sources
            .iter()
            .map(|(p, s)| (files::normalize(p), s.as_str()))
            .collect();
        Self::from_loader(files, import_paths, &|p: &str| {
            by_path.get(p).map(|s| (*s).to_owned())
        })
    }

    /// Like [`Descriptors::from_files`], reading each source on demand (`load` gets a
    /// normalized path relative to the collection root and returns `None` if there is no
    /// such file). Desktop and CLI read from disk this way, so imports need no list.
    pub fn from_loader(
        files: &[String],
        import_paths: &[String],
        load: &Loader<'_>,
    ) -> Result<Self, GrpcError> {
        Ok(Self {
            pool: files::compile(files, import_paths, load)?,
        })
    }

    /// Asks the server at `url` (`grpc://`, `grpcs://`, `http(s)://` or `host:port`; a path
    /// is a prefix of the reflection method) with `metadata`.
    pub async fn from_reflection<S: StreamTransport + ?Sized>(
        transport: &S,
        url: &str,
        metadata: &[(String, String)],
    ) -> Result<Self, GrpcError> {
        Ok(Self {
            pool: reflection::fetch(transport, url, metadata).await?,
        })
    }

    /// Services and their methods, sorted by name; the reflection service itself is left out.
    pub fn services(&self) -> Vec<GrpcService> {
        let mut services: Vec<GrpcService> = self
            .pool
            .services()
            .filter(|s| !s.full_name().starts_with("grpc.reflection."))
            .map(|s| GrpcService {
                name: s.full_name().to_owned(),
                methods: s.methods().map(|m| method_info(&m)).collect(),
            })
            .collect();
        services.sort_by(|a, b| a.name.cmp(&b.name));
        services
    }

    /// The method at `path` (`/package.Service/Method`; a URL prefix before the service is
    /// ignored).
    pub fn method(&self, path: &str) -> Result<GrpcMethod, GrpcError> {
        self.find(path).map(|m| method_info(&m))
    }

    fn find(&self, path: &str) -> Result<MethodDescriptor, GrpcError> {
        let unknown = || GrpcError::UnknownMethod(path.to_owned());
        let trimmed = path.trim().trim_matches('/');
        let (service, method) = trimmed.rsplit_once('/').ok_or_else(unknown)?;
        let service = service.rsplit('/').next().unwrap_or(service);
        self.pool
            .get_service_by_name(service)
            .and_then(|s| s.methods().find(|m| m.name() == method))
            .ok_or_else(unknown)
    }

    /// JSON → protobuf bytes of the input message of `method` (path).
    pub fn encode(&self, method: &str, json: &str) -> Result<Vec<u8>, GrpcError> {
        json::encode(self.find(method)?.input(), json)
    }

    /// Protobuf bytes of the output message of `method` → JSON (pretty).
    pub fn decode(&self, method: &str, bytes: &[u8]) -> Result<String, GrpcError> {
        json::decode(self.find(method)?.output(), bytes)
    }

    /// Protobuf bytes of the input message of `method` → JSON (pretty), for sent messages.
    pub fn decode_input(&self, method: &str, bytes: &[u8]) -> Result<String, GrpcError> {
        json::decode(self.find(method)?.input(), bytes)
    }

    /// A JSON skeleton of the input message (every field with its default).
    pub fn template(&self, method: &str) -> Result<String, GrpcError> {
        Ok(json::template(&self.find(method)?.input()))
    }

    /// The frame to send for a JSON message.
    pub fn encode_frame(&self, method: &str, json: &str) -> Result<Frame, GrpcError> {
        self.encode(method, json).map(Frame::Binary)
    }

    /// A session event with protobuf messages shown as JSON text: binary frames become text
    /// frames (received: output type, sent: input type) named after their message type. A
    /// frame that does not decode stays binary; everything else is unchanged.
    pub fn decode_event(&self, method: &str, event: StreamEvent) -> StreamEvent {
        let StreamEvent::Message {
            at_ms,
            direction,
            frame: Frame::Binary(bytes),
            name,
            id,
        } = event
        else {
            return event;
        };
        let Ok(found) = self.find(method) else {
            return StreamEvent::Message {
                at_ms,
                direction,
                frame: Frame::Binary(bytes),
                name,
                id,
            };
        };
        let desc = match direction {
            rt_core::stream::Direction::Received => found.output(),
            rt_core::stream::Direction::Sent => found.input(),
        };
        match json::decode(desc.clone(), &bytes) {
            Ok(text) => StreamEvent::Message {
                at_ms,
                direction,
                frame: Frame::Text(text),
                name: Some(desc.full_name().to_owned()),
                id,
            },
            Err(_) => StreamEvent::Message {
                at_ms,
                direction,
                frame: Frame::Binary(bytes),
                name,
                id,
            },
        }
    }
}

fn method_info(m: &MethodDescriptor) -> GrpcMethod {
    let method_type = match (m.is_client_streaming(), m.is_server_streaming()) {
        (false, false) => GrpcMethodType::Unary,
        (true, false) => GrpcMethodType::ClientStreaming,
        (false, true) => GrpcMethodType::ServerStreaming,
        (true, true) => GrpcMethodType::BidiStreaming,
    };
    GrpcMethod {
        name: m.name().to_owned(),
        path: format!("/{}/{}", m.parent_service().full_name(), m.name()),
        method_type,
        input_type: m.input().full_name().to_owned(),
        output_type: m.output().full_name().to_owned(),
    }
}
