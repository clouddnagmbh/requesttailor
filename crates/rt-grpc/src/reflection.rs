// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! gRPC server reflection client: `grpc.reflection.v1.ServerReflection/ServerReflectionInfo`,
//! falling back to `v1alpha` when the server does not implement `v1`.
//!
//! One bidi stream: `list_services`, then `file_containing_symbol` for every service, then
//! `file_by_filename` for every dependency the server has not sent yet. Well-known types the
//! server does not serve come from the built-in copies.

use std::collections::{BTreeMap, BTreeSet};

use prost_reflect::prost::Message as _;
use prost_reflect::prost_types::FileDescriptorProto;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, Value};
use protox::file::{FileResolver, GoogleFileResolver};
use rt_core::stream::{Frame, StreamEvent, StreamOpen, StreamSession, StreamTransport};

use crate::GrpcError;

const PROTO: &str = include_str!("../proto/reflection.proto");
/// gRPC status `UNIMPLEMENTED`.
const UNIMPLEMENTED: u32 = 12;
/// How long reflection may take as a whole.
const TIMEOUT_MS: u64 = 30_000;

enum Failure {
    /// The server does not implement this version.
    Unimplemented,
    Other(GrpcError),
}

impl From<GrpcError> for Failure {
    fn from(e: GrpcError) -> Self {
        Failure::Other(e)
    }
}

fn reflection_error(message: impl Into<String>) -> Failure {
    Failure::Other(GrpcError::Reflection(message.into()))
}

/// The reflection messages for `version` (`v1` or `v1alpha`).
fn messages(version: &str) -> Result<(MessageDescriptor, MessageDescriptor), GrpcError> {
    let source = PROTO.replace(
        "package grpc.reflection.v1;",
        &format!("package grpc.reflection.{version};"),
    );
    let name = "reflection.proto".to_owned();
    let pool = crate::files::compile(std::slice::from_ref(&name), &[], &|p: &str| {
        (p == name).then(|| source.clone())
    })?;
    let get = |m: &str| {
        pool.get_message_by_name(&format!("grpc.reflection.{version}.{m}"))
            .ok_or_else(|| GrpcError::Reflection(format!("internal: no {m}")))
    };
    Ok((
        get("ServerReflectionRequest")?,
        get("ServerReflectionResponse")?,
    ))
}

pub(crate) async fn fetch<S: StreamTransport + ?Sized>(
    transport: &S,
    url: &str,
    metadata: &[(String, String)],
) -> Result<DescriptorPool, GrpcError> {
    match fetch_version(transport, url, metadata, "v1").await {
        Ok(pool) => Ok(pool),
        Err(Failure::Unimplemented) => {
            match fetch_version(transport, url, metadata, "v1alpha").await {
                Ok(pool) => Ok(pool),
                Err(Failure::Unimplemented) => Err(GrpcError::Reflection(
                    "the server does not implement server reflection (v1 or v1alpha)".into(),
                )),
                Err(Failure::Other(e)) => Err(e),
            }
        }
        Err(Failure::Other(e)) => Err(e),
    }
}

struct Client<'s> {
    session: Box<dyn StreamSession + 's>,
    request: MessageDescriptor,
    response: MessageDescriptor,
}

impl Client<'_> {
    async fn ask(&mut self, field: &str, value: &str) -> Result<DynamicMessage, Failure> {
        let mut request = DynamicMessage::new(self.request.clone());
        request.set_field_by_name(field, Value::String(value.to_owned()));
        self.session
            .send(Frame::Binary(request.encode_to_vec()))
            .await
            .map_err(|e| reflection_error(e.to_string()))?;
        loop {
            match self.session.next().await {
                Some(StreamEvent::Opened { .. }) => {}
                Some(StreamEvent::Message {
                    frame: Frame::Binary(bytes),
                    ..
                }) => {
                    let response = DynamicMessage::decode(self.response.clone(), bytes.as_slice())
                        .map_err(|e| reflection_error(format!("invalid response: {e}")))?;
                    return Ok(response);
                }
                Some(StreamEvent::Message { .. }) => {
                    return Err(reflection_error("unexpected text frame"));
                }
                Some(StreamEvent::Closed { code, reason, .. }) => {
                    return Err(match code {
                        Some(UNIMPLEMENTED) => Failure::Unimplemented,
                        Some(code) => reflection_error(format!("status {code}: {reason}")),
                        None => reflection_error(format!("the server closed the stream {reason}")),
                    });
                }
                Some(StreamEvent::Error { error, .. }) => {
                    return Err(reflection_error(error.to_string()));
                }
                None => return Err(reflection_error("the stream ended without an answer")),
            }
        }
    }
}

/// The error response of `response`, if it is one.
fn error_of(response: &DynamicMessage) -> Option<String> {
    if !response.has_field_by_name("error_response") {
        return None;
    }
    let error = response.get_field_by_name("error_response")?;
    let error = error.as_message()?;
    let code = error
        .get_field_by_name("error_code")
        .and_then(|v| v.as_i32())
        .unwrap_or(0);
    let message = error
        .get_field_by_name("error_message")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    Some(format!("status {code}: {message}"))
}

fn file_protos(response: &DynamicMessage) -> Result<Vec<FileDescriptorProto>, Failure> {
    if let Some(error) = error_of(response) {
        return Err(reflection_error(error));
    }
    let files = response
        .get_field_by_name("file_descriptor_response")
        .and_then(|v| {
            v.as_message()?
                .get_field_by_name("file_descriptor_proto")
                .map(|l| l.into_owned())
        });
    let Some(Value::List(files)) = files else {
        return Err(reflection_error("no file descriptors in the answer"));
    };
    files
        .iter()
        .filter_map(Value::as_bytes)
        .map(|b| {
            FileDescriptorProto::decode(b.as_ref())
                .map_err(|e| reflection_error(format!("invalid file descriptor: {e}")))
        })
        .collect()
}

async fn fetch_version<S: StreamTransport + ?Sized>(
    transport: &S,
    url: &str,
    metadata: &[(String, String)],
    version: &str,
) -> Result<DescriptorPool, Failure> {
    let (request, response) = messages(version)?;
    let method = format!("/grpc.reflection.{version}.ServerReflection/ServerReflectionInfo");
    let (base, path) = rt_core::stream::grpc_target(url, &method).map_err(reflection_error)?;
    let session = transport
        .open(StreamOpen::Grpc {
            url: base,
            path,
            metadata: metadata.to_vec(),
            timeout_ms: Some(TIMEOUT_MS),
        })
        .await
        .map_err(|e| reflection_error(e.to_string()))?;
    let mut client = Client {
        session,
        request,
        response,
    };
    let result = collect(&mut client).await;
    let _ = client.session.finish().await;
    client.session.close().await;
    result
}

async fn collect(client: &mut Client<'_>) -> Result<DescriptorPool, Failure> {
    let listed = client.ask("list_services", "*").await?;
    if let Some(error) = error_of(&listed) {
        return Err(reflection_error(error));
    }
    let services: Vec<String> = listed
        .get_field_by_name("list_services_response")
        .and_then(|v| {
            let list = v.as_message()?.get_field_by_name("service")?;
            let Value::List(items) = list.as_ref() else {
                return None;
            };
            Some(
                items
                    .iter()
                    .filter_map(|s| {
                        s.as_message()?
                            .get_field_by_name("name")?
                            .as_str()
                            .map(str::to_owned)
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();

    let mut files: BTreeMap<String, FileDescriptorProto> = BTreeMap::new();
    for service in services
        .iter()
        .filter(|s| !s.starts_with("grpc.reflection."))
    {
        for file in file_protos(&client.ask("file_containing_symbol", service).await?)? {
            files.insert(file.name().to_owned(), file);
        }
    }
    // Dependencies the server did not send along.
    let google = GoogleFileResolver::new();
    let mut failed: BTreeSet<String> = BTreeSet::new();
    loop {
        let missing: Vec<String> = files
            .values()
            .flat_map(|f| f.dependency.iter().cloned())
            .filter(|d| !files.contains_key(d) && !failed.contains(d))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if missing.is_empty() {
            break;
        }
        for name in missing {
            if let Ok(built_in) = google.open_file(&name) {
                files.insert(name, built_in.file_descriptor_proto().clone());
                continue;
            }
            match file_protos(&client.ask("file_by_filename", &name).await?) {
                Ok(found) => {
                    for file in found {
                        files.insert(file.name().to_owned(), file);
                    }
                    if !files.contains_key(&name) {
                        failed.insert(name);
                    }
                }
                Err(Failure::Other(_)) => {
                    failed.insert(name);
                }
                Err(Failure::Unimplemented) => return Err(Failure::Unimplemented),
            }
        }
    }
    if let Some(name) = failed.first() {
        return Err(reflection_error(format!(
            "the server did not send the file {name}"
        )));
    }
    let mut pool = DescriptorPool::new();
    pool.add_file_descriptor_protos(topological(files))
        .map_err(|e| reflection_error(format!("invalid descriptors: {e}")))?;
    Ok(pool)
}

/// Dependencies first.
fn topological(mut files: BTreeMap<String, FileDescriptorProto>) -> Vec<FileDescriptorProto> {
    fn visit(
        name: &str,
        files: &mut BTreeMap<String, FileDescriptorProto>,
        out: &mut Vec<FileDescriptorProto>,
    ) {
        let Some(file) = files.remove(name) else {
            return;
        };
        for dep in &file.dependency {
            visit(dep, files, out);
        }
        out.push(file);
    }
    let names: Vec<String> = files.keys().cloned().collect();
    let mut out = Vec::new();
    for name in names {
        visit(&name, &mut files, &mut out);
    }
    out
}
