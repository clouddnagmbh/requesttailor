// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send` on a gRPC request (wave 8, strand B): descriptors from the request's proto
//! file, the collection's `config.protobuf` or server reflection; the selected messages (all
//! messages if none is selected; unary and server-streaming methods send the first) are
//! encoded from JSON and sent through `rt_core::stream::open`, then the client half-closes
//! and the masked `StreamLog` is printed with every message decoded to JSON.
//!
//! Exit codes: 0 when the call ends with status 0, 1 (transport) for any other status or a
//! transport error, 3 (prepare) when the descriptors or a message cannot be built.

use rt_core::model::{GrpcMethodType, GrpcSpec, Protocol};
use rt_core::prepare::{FileSource, PrepareInput, prepare};
use rt_core::send::SendOptions;
use rt_core::stream::{StreamEvent, StreamLog, StreamTransport};
use rt_format::CollectionDoc;
use rt_grpc::{Descriptors, ProtoSource};

use crate::{DirFiles, EXIT_OK, EXIT_PREPARE, EXIT_TRANSPORT, SendArgs, secrets, services};

/// The gRPC part of the request at `path`, if it is a gRPC request.
pub fn spec(doc: &CollectionDoc, path: &str) -> Option<GrpcSpec> {
    match &doc.model().request(path)?.request.spec.protocol {
        Protocol::Grpc(g) => Some((**g).clone()),
        _ => None,
    }
}

/// Sends the gRPC request; returns the log (decoded) or why nothing was sent, and the exit
/// code.
pub async fn send<T: StreamTransport>(
    doc: &CollectionDoc,
    path: &str,
    spec: &GrpcSpec,
    args: &SendArgs,
    files: &DirFiles,
    transport: &T,
) -> Result<(StreamLog, i32), (String, i32)> {
    let prepare_err = |e: String| (e, EXIT_PREPARE);
    let collection = secrets::resolve(doc.model(), args.environment.as_deref(), &args.vars)
        .await
        .map_err(prepare_err)?;
    let context = collection
        .request(path)
        .ok_or_else(|| (format!("request not found: {path}"), crate::EXIT_USAGE))?;
    let descriptors = match rt_grpc::proto_source(&collection.config.protobuf, path, spec) {
        ProtoSource::Files {
            files: protos,
            import_paths,
        } => Descriptors::from_loader(&protos, &import_paths, &|p: &str| {
            files
                .read(p)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        })
        .map_err(|e| prepare_err(e.to_string()))?,
        ProtoSource::Reflection => {
            let input = PrepareInput {
                environment: args.environment.as_deref(),
                globals: Vec::new(),
                runtime: args.vars.clone(),
            };
            let prepared =
                prepare(&context, &input, files).map_err(|e| prepare_err(e.to_string()))?;
            Descriptors::from_reflection(
                transport,
                &prepared.request.url,
                &prepared.request.headers,
            )
            .await
            .map_err(|e| (e.to_string(), EXIT_TRANSPORT))?
        }
    };
    let method = descriptors
        .method(&spec.method)
        .map_err(|e| prepare_err(e.to_string()))?;
    let mut messages: Vec<&str> = spec
        .messages
        .iter()
        .filter(|m| m.selected)
        .map(|m| m.data.as_str())
        .collect();
    if messages.is_empty() {
        messages = spec.messages.iter().map(|m| m.data.as_str()).collect();
    }
    if matches!(
        method.method_type,
        GrpcMethodType::Unary | GrpcMethodType::ServerStreaming
    ) {
        messages.truncate(1);
        if messages.is_empty() {
            messages.push("{}");
        }
    }
    let frames = messages
        .iter()
        .map(|m| descriptors.encode_frame(&spec.method, m))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| prepare_err(e.to_string()))?;

    let cli = services::CliServices::new(args, files.root()).map_err(prepare_err)?;
    let options = SendOptions {
        iteration: None,
        body_limit: args.body_limit,
    };
    let mut session = rt_core::stream::open(&context, &options, &cli.services(), transport)
        .await
        .map_err(|e| (format!("{e:?}"), EXIT_PREPARE))?;
    for frame in frames {
        if let Err(e) = session.send(frame).await {
            session.log.events.push(StreamEvent::Error {
                at_ms: session.started.elapsed().as_millis() as u64,
                error: e,
            });
            break;
        }
    }
    let _ = session.inner.finish().await;
    while session.next().await.is_some() {}
    let mut log = std::mem::take(&mut session.log);
    log.events = log
        .events
        .into_iter()
        .map(|e| descriptors.decode_event(&spec.method, e))
        .collect();
    let code = match log.events.last() {
        Some(StreamEvent::Closed { code: Some(0), .. }) => EXIT_OK,
        _ => EXIT_TRANSPORT,
    };
    Ok((log, code))
}
