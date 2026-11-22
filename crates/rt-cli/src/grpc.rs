// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! gRPC in `rt send` (wave 8, strand B), used by the session runner (`stream.rs`):
//! descriptors from the request's proto file, the collection's `config.protobuf` or server
//! reflection; messages are JSON encoded to protobuf, and every message in the printed
//! `StreamLog` is decoded back to JSON.

use rt_core::model::{Collection, GrpcSpec};
use rt_core::prepare::{FileSource, PrepareInput, prepare};
use rt_core::send::SendError;
use rt_core::stream::{Frame, StreamTransport};
use rt_core::transport::TransportError;
use rt_grpc::{Descriptors, GrpcError, ProtoSource};

use crate::{DirFiles, SendArgs};

fn send_error(e: GrpcError) -> SendError {
    match e {
        GrpcError::Reflection(m) => SendError::Transport(TransportError::Other(format!(
            "gRPC server reflection failed: {m}"
        ))),
        other => SendError::Prepare(other.to_string()),
    }
}

/// The descriptors of the gRPC request at `path` of `collection` (secrets resolved); the
/// method must be one of them.
pub async fn descriptors<S: StreamTransport + ?Sized>(
    collection: &Collection,
    path: &str,
    spec: &GrpcSpec,
    args: &SendArgs,
    files: &DirFiles,
    transport: &S,
) -> Result<Descriptors, SendError> {
    let d = match rt_grpc::proto_source(&collection.config.protobuf, path, spec) {
        ProtoSource::Files {
            files: protos,
            import_paths,
        } => Descriptors::from_loader(&protos, &import_paths, &|p: &str| {
            files
                .read(p)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        })
        .map_err(send_error)?,
        ProtoSource::Reflection => {
            let context = collection
                .request(path)
                .ok_or_else(|| SendError::NotFound(path.to_owned()))?;
            let input = PrepareInput {
                environment: args.environment.as_deref(),
                globals: Vec::new(),
                runtime: args.vars.clone(),
            };
            let prepared =
                prepare(&context, &input, files).map_err(|e| SendError::Prepare(e.to_string()))?;
            Descriptors::from_reflection(
                transport,
                &prepared.request.url,
                &prepared.request.headers,
            )
            .await
            .map_err(send_error)?
        }
    };
    d.method(&spec.method).map_err(send_error)?;
    Ok(d)
}

/// The protobuf frame of a message whose variables are resolved (`frame` is what
/// `Session::frame` made of it).
pub fn encode(d: &Descriptors, method: &str, frame: Frame) -> Result<Frame, String> {
    match frame {
        Frame::Text(json) => d.encode_frame(method, &json).map_err(|e| e.to_string()),
        Frame::Binary(_) => Err("gRPC messages are JSON; binary messages are not supported".into()),
    }
}
