// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! gRPC test server (wave 8, strand B): cleartext HTTP/2 (prior knowledge) with the echo
//! service of `testdata/grpc/protos/echo.proto` and server reflection (`v1` and `v1alpha`).
//! Test infrastructure only.
//!
//! | Method | Behaviour |
//! | --- | --- |
//! | `rt.echo.v1.EchoService/Echo` | the message back; `metadata.entries` has every request header starting with `x-`; response header `x-echo-header`, trailer `x-echo-trailer: done`; request header `x-compress: 1` makes the reply "compressed" (flag 1, `grpc-encoding: gzip`) |
//! | `…/Countdown` | `count` replies (default 3) `"{message} {i}"` with `index` i, `delay_ms` apart |
//! | `…/Collect` | after the client's end: one reply, the messages joined by spaces, `index` = their count |
//! | `…/Chat` | one reply per message as it arrives (`index` counts from 0) |
//! | `…/Fail` | trailers-only response with `grpc-status: code`, `grpc-message: message` |
//! | other paths | `grpc-status: 12` (UNIMPLEMENTED), trailers-only |
//! | `grpc.reflection.{v1,v1alpha}.ServerReflection/ServerReflectionInfo` | list services, file by name, file containing a symbol (that file only, dependencies by name) |

use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::OnceLock;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Body as HttpBody, Frame, Incoming};
use hyper::header::HeaderValue;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use prost_reflect::prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage, MapKey, MessageDescriptor, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

type Body = BoxBody<Bytes, Infallible>;

/// Default port of `rt-testserver --grpc-port` in scripts (browser tests via the companion).
pub const DEFAULT_GRPC_PORT: u16 = 47812;

const ECHO: &str = include_str!("../../../testdata/grpc/protos/echo.proto");
const COMMON: &str = include_str!("../../../testdata/grpc/include/rt/common.proto");
const REFLECTION: &str = include_str!("../../rt-grpc/proto/reflection.proto");

/// Which reflection versions the server offers (both by default).
#[derive(Debug, Clone, Copy)]
pub struct GrpcOptions {
    pub reflection_v1: bool,
    pub reflection_v1alpha: bool,
}

impl Default for GrpcOptions {
    fn default() -> Self {
        Self {
            reflection_v1: true,
            reflection_v1alpha: true,
        }
    }
}

/// Binds `addr` (port 0: a free port) and serves the echo service until the process ends.
pub async fn spawn_grpc(addr: SocketAddr) -> std::io::Result<SocketAddr> {
    spawn_grpc_with(addr, GrpcOptions::default()).await
}

pub async fn spawn_grpc_with(
    addr: SocketAddr,
    options: GrpcOptions,
) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    pool();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| async move {
                    Ok::<_, Infallible>(handle(req, options).await)
                });
                let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    Ok(local)
}

/// The echo service, the common types, the well-known types they use, and both reflection
/// versions, compiled once.
fn pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let v1alpha = REFLECTION.replace(
            "package grpc.reflection.v1;",
            "package grpc.reflection.v1alpha;",
        );
        let files: Vec<(&str, String)> = vec![
            ("echo.proto", ECHO.to_owned()),
            ("rt/common.proto", COMMON.to_owned()),
            ("grpc/reflection/v1/reflection.proto", REFLECTION.to_owned()),
            ("grpc/reflection/v1alpha/reflection.proto", v1alpha),
        ];
        let mut compiler = protox::Compiler::with_file_resolver(Sources(files.clone()));
        compiler.include_imports(true);
        for (name, _) in &files {
            compiler.open_file(name).expect("test protos compile");
        }
        compiler.descriptor_pool()
    })
}

struct Sources(Vec<(&'static str, String)>);

impl protox::file::FileResolver for Sources {
    fn open_file(&self, name: &str) -> Result<protox::file::File, protox::Error> {
        if let Some((_, source)) = self.0.iter().find(|(n, _)| *n == name) {
            return protox::file::File::from_source(name, source);
        }
        protox::file::GoogleFileResolver::new().open_file(name)
    }
}

fn message(name: &str) -> MessageDescriptor {
    pool()
        .get_message_by_name(name)
        .unwrap_or_else(|| panic!("no message {name}"))
}

async fn handle(req: Request<Incoming>, options: GrpcOptions) -> Response<Body> {
    let path = req.uri().path().to_owned();
    let is_grpc = req
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/grpc"));
    if !is_grpc {
        return Response::builder()
            .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .body(Full::new(Bytes::from_static(b"gRPC only")).boxed())
            .expect("valid response");
    }
    let headers = req.headers().clone();
    let body = req.into_body();
    match path.as_str() {
        "/rt.echo.v1.EchoService/Echo" => {
            let compress = headers
                .get("x-compress")
                .is_some_and(|v| v.as_bytes() == b"1");
            let request_headers = headers.clone();
            streaming(&headers, compress, move |tx| {
                echo(body, request_headers, tx, compress)
            })
        }
        "/rt.echo.v1.EchoService/Countdown" => {
            streaming(&headers, false, move |tx| countdown(body, tx))
        }
        "/rt.echo.v1.EchoService/Collect" => {
            streaming(&headers, false, move |tx| collect(body, tx))
        }
        "/rt.echo.v1.EchoService/Chat" => streaming(&headers, false, move |tx| chat(body, tx)),
        "/rt.echo.v1.EchoService/Fail" => {
            // Trailers-only: read the request, then answer with the status alone.
            let mut reader = Reader::new(body);
            let request = reader.next(&message("rt.echo.v1.FailRequest")).await;
            let (code, text) = request.map_or((3, "no request".to_owned()), |m| {
                (
                    m.get_field_by_name("code")
                        .and_then(|v| v.as_i32())
                        .unwrap_or(0),
                    text_field(&m, "message"),
                )
            });
            trailers_only(code, &text)
        }
        "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo" if options.reflection_v1 => {
            streaming(&headers, false, move |tx| reflection(body, tx, "v1"))
        }
        "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"
            if options.reflection_v1alpha =>
        {
            streaming(&headers, false, move |tx| reflection(body, tx, "v1alpha"))
        }
        _ => trailers_only(12, &format!("unknown method {path}")),
    }
}

/// A response whose body `run` writes through the channel (messages, then trailers).
fn streaming<F, Fut>(request_headers: &HeaderMap, compress: bool, run: F) -> Response<Body>
where
    F: FnOnce(Out) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = mpsc::channel(16);
    tokio::spawn(run(Out(tx)));
    let mut response = Response::new(ChannelBody(rx).boxed());
    let h = response.headers_mut();
    h.insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/grpc"),
    );
    let echoed = request_headers
        .get("x-trace")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("echo"));
    h.insert("x-echo-header", echoed);
    if compress {
        h.insert("grpc-encoding", HeaderValue::from_static("gzip"));
    }
    response
}

fn trailers_only(code: i32, text: &str) -> Response<Body> {
    let mut response = Response::new(Full::new(Bytes::new()).boxed());
    let h = response.headers_mut();
    h.insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/grpc"),
    );
    h.insert("grpc-status", HeaderValue::from(code));
    if let Ok(v) = HeaderValue::from_str(&encode_message(text)) {
        h.insert("grpc-message", v);
    }
    response
}

/// `grpc-message`: printable ASCII except `%` as is, everything else percent-encoded.
fn encode_message(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if (b' '..=b'~').contains(&b) && b != b'%' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Response messages and the final status.
struct Out(mpsc::Sender<Frame<Bytes>>);

impl Out {
    async fn message(&self, message: &DynamicMessage, compressed: bool) -> bool {
        let bytes = message.encode_to_vec();
        let mut framed = Vec::with_capacity(bytes.len() + 5);
        framed.push(u8::from(compressed));
        framed.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        framed.extend_from_slice(&bytes);
        self.0.send(Frame::data(Bytes::from(framed))).await.is_ok()
    }

    async fn status(self, code: i32, text: &str, extra: &[(&'static str, &str)]) {
        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", HeaderValue::from(code));
        if !text.is_empty()
            && let Ok(v) = HeaderValue::from_str(&encode_message(text))
        {
            trailers.insert("grpc-message", v);
        }
        for (name, value) in extra {
            if let Ok(v) = HeaderValue::from_str(value) {
                trailers.insert(*name, v);
            }
        }
        let _ = self.0.send(Frame::trailers(trailers)).await;
    }
}

struct ChannelBody(mpsc::Receiver<Frame<Bytes>>);

impl HttpBody for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.0.poll_recv(cx).map(|f| f.map(Ok))
    }
}

/// Reads length-prefixed request messages.
struct Reader {
    body: Incoming,
    buffer: Vec<u8>,
    ended: bool,
}

impl Reader {
    fn new(body: Incoming) -> Self {
        Self {
            body,
            buffer: Vec::new(),
            ended: false,
        }
    }

    /// The next message, `None` at the end of the client's stream (or on garbage).
    async fn next(&mut self, desc: &MessageDescriptor) -> Option<DynamicMessage> {
        loop {
            if self.buffer.len() >= 5 {
                let len = u32::from_be_bytes([
                    self.buffer[1],
                    self.buffer[2],
                    self.buffer[3],
                    self.buffer[4],
                ]) as usize;
                if self.buffer.len() >= 5 + len {
                    let bytes: Vec<u8> = self.buffer.drain(..5 + len).skip(5).collect();
                    return DynamicMessage::decode(desc.clone(), bytes.as_slice()).ok();
                }
            }
            if self.ended {
                return None;
            }
            match self.body.frame().await {
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        self.buffer.extend_from_slice(&data);
                    }
                }
                _ => self.ended = true,
            }
        }
    }
}

fn text_field(m: &DynamicMessage, name: &str) -> String {
    m.get_field_by_name(name)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn reply(text: &str, index: i32, metadata: Option<Vec<(String, String)>>) -> DynamicMessage {
    let mut m = DynamicMessage::new(message("rt.echo.v1.EchoReply"));
    m.set_field_by_name("message", Value::String(text.to_owned()));
    m.set_field_by_name("index", Value::I32(index));
    if let Some(entries) = metadata {
        let mut meta = DynamicMessage::new(message("rt.common.Metadata"));
        meta.set_field_by_name(
            "entries",
            Value::Map(
                entries
                    .into_iter()
                    .map(|(k, v)| (MapKey::String(k), Value::String(v)))
                    .collect(),
            ),
        );
        m.set_field_by_name("metadata", Value::Message(meta));
    }
    m
}

async fn echo(body: Incoming, headers: HeaderMap, tx: Out, compress: bool) {
    let mut reader = Reader::new(body);
    let Some(request) = reader.next(&message("rt.echo.v1.EchoRequest")).await else {
        tx.status(3, "Echo needs one message", &[]).await;
        return;
    };
    let metadata: Vec<(String, String)> = headers
        .iter()
        .filter(|(k, _)| k.as_str().starts_with("x-"))
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    tx.message(
        &reply(&text_field(&request, "message"), 0, Some(metadata)),
        compress,
    )
    .await;
    tx.status(0, "", &[("x-echo-trailer", "done")]).await;
}

async fn countdown(body: Incoming, tx: Out) {
    let mut reader = Reader::new(body);
    let Some(request) = reader.next(&message("rt.echo.v1.CountdownRequest")).await else {
        tx.status(3, "Countdown needs one message", &[]).await;
        return;
    };
    let text = text_field(&request, "message");
    let count = request
        .get_field_by_name("count")
        .and_then(|v| v.as_i32())
        .filter(|c| *c > 0)
        .unwrap_or(3);
    let delay = request
        .get_field_by_name("delay_ms")
        .and_then(|v| v.as_i32())
        .unwrap_or(0)
        .clamp(0, 60_000) as u64;
    for i in 0..count {
        if i > 0 && delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if !tx
            .message(&reply(&format!("{text} {i}"), i, None), false)
            .await
        {
            return;
        }
    }
    tx.status(0, "", &[]).await;
}

async fn collect(body: Incoming, tx: Out) {
    let mut reader = Reader::new(body);
    let desc = message("rt.echo.v1.EchoRequest");
    let mut texts = Vec::new();
    while let Some(m) = reader.next(&desc).await {
        texts.push(text_field(&m, "message"));
    }
    tx.message(&reply(&texts.join(" "), texts.len() as i32, None), false)
        .await;
    tx.status(0, "", &[]).await;
}

async fn chat(body: Incoming, tx: Out) {
    let mut reader = Reader::new(body);
    let desc = message("rt.echo.v1.EchoRequest");
    let mut index = 0;
    while let Some(m) = reader.next(&desc).await {
        if !tx
            .message(&reply(&text_field(&m, "message"), index, None), false)
            .await
        {
            return;
        }
        index += 1;
    }
    tx.status(0, "", &[]).await;
}

async fn reflection(body: Incoming, tx: Out, version: &'static str) {
    let mut reader = Reader::new(body);
    let request_desc = message(&format!(
        "grpc.reflection.{version}.ServerReflectionRequest"
    ));
    let response_desc = message(&format!(
        "grpc.reflection.{version}.ServerReflectionResponse"
    ));
    let name = |m: &str| format!("grpc.reflection.{version}.{m}");
    while let Some(request) = reader.next(&request_desc).await {
        let mut response = DynamicMessage::new(response_desc.clone());
        response.set_field_by_name("original_request", Value::Message(request.clone()));
        let files = |file: Option<prost_reflect::FileDescriptor>| -> Result<Value, String> {
            let file = file.ok_or("not found")?;
            Ok(Value::List(vec![Value::Bytes(Bytes::from(
                file.file_descriptor_proto().encode_to_vec(),
            ))]))
        };
        let answer: Result<(&str, Value), String> = if request.has_field_by_name("list_services") {
            let services: Vec<Value> = pool()
                .services()
                .filter(|s| match s.full_name() {
                    "grpc.reflection.v1.ServerReflection" => version == "v1",
                    "grpc.reflection.v1alpha.ServerReflection" => version == "v1alpha",
                    _ => true,
                })
                .map(|s| {
                    let mut entry = DynamicMessage::new(message(&name("ServiceResponse")));
                    entry.set_field_by_name("name", Value::String(s.full_name().to_owned()));
                    Value::Message(entry)
                })
                .collect();
            let mut list = DynamicMessage::new(message(&name("ListServiceResponse")));
            list.set_field_by_name("service", Value::List(services));
            Ok(("list_services_response", Value::Message(list)))
        } else if request.has_field_by_name("file_by_filename") {
            let file = pool().get_file_by_name(&text_field(&request, "file_by_filename"));
            files(file).map(|v| ("file_descriptor_response", v))
        } else if request.has_field_by_name("file_containing_symbol") {
            let symbol = text_field(&request, "file_containing_symbol");
            let file = pool()
                .get_service_by_name(&symbol)
                .map(|s| s.parent_file())
                .or_else(|| pool().get_message_by_name(&symbol).map(|m| m.parent_file()));
            files(file).map(|v| ("file_descriptor_response", v))
        } else {
            Err("unsupported request".into())
        };
        match answer {
            Ok(("file_descriptor_response", list)) => {
                let mut fd = DynamicMessage::new(message(&name("FileDescriptorResponse")));
                fd.set_field_by_name("file_descriptor_proto", list);
                response.set_field_by_name("file_descriptor_response", Value::Message(fd));
            }
            Ok((field, value)) => response.set_field_by_name(field, value),
            Err(text) => {
                let mut error = DynamicMessage::new(message(&name("ErrorResponse")));
                error.set_field_by_name("error_code", Value::I32(5));
                error.set_field_by_name("error_message", Value::String(text));
                response.set_field_by_name("error_response", Value::Message(error));
            }
        }
        if !tx.message(&response, false).await {
            return;
        }
    }
    tx.status(0, "", &[]).await;
}
