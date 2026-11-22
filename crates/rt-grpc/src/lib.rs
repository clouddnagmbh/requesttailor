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
//!
//! Builds for every target, wasm32 and iOS included.

use rt_core::model::GrpcMethodType;
use rt_core::stream::StreamTransport;
use serde::{Deserialize, Serialize};

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
    _private: (),
}

/// Proto sources by path relative to the collection root (`/`-separated); the caller reads
/// them (desktop and CLI from disk, web from the opened folder).
pub type ProtoSources = Vec<(String, String)>;

impl Descriptors {
    /// Compiles `files` (paths into `sources`) with `import_paths`. Strand B.
    pub fn from_files(
        _sources: &ProtoSources,
        _files: &[String],
        _import_paths: &[String],
    ) -> Result<Self, GrpcError> {
        Err(GrpcError::Proto("gRPC arrives in wave 8, strand B".into()))
    }

    /// Asks the server at `url` (`http`/`https`) with `metadata`. Strand B.
    pub async fn from_reflection<S: StreamTransport + ?Sized>(
        _transport: &S,
        _url: &str,
        _metadata: &[(String, String)],
    ) -> Result<Self, GrpcError> {
        Err(GrpcError::Reflection(
            "gRPC arrives in wave 8, strand B".into(),
        ))
    }

    pub fn services(&self) -> Vec<GrpcService> {
        Vec::new()
    }

    /// JSON → protobuf bytes of the input message of `method` (path). Strand B.
    pub fn encode(&self, method: &str, _json: &str) -> Result<Vec<u8>, GrpcError> {
        Err(GrpcError::UnknownMethod(method.into()))
    }

    /// Protobuf bytes of the output message of `method` → JSON (pretty). Strand B.
    pub fn decode(&self, method: &str, _bytes: &[u8]) -> Result<String, GrpcError> {
        Err(GrpcError::UnknownMethod(method.into()))
    }

    /// A JSON skeleton of the input message (every field with its default). Strand B.
    pub fn template(&self, method: &str) -> Result<String, GrpcError> {
        Err(GrpcError::UnknownMethod(method.into()))
    }
}
