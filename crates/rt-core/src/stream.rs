// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sessions (wave 8): WebSocket, Server-Sent Events and gRPC. Unlike [`crate::send`], a
//! session stays open, messages go both ways, and the result is a message history.
//!
//! Layers, the same as for HTTP:
//! - [`open`] prepares the request (variables, inheritance, auth, OAuth token, plugin
//!   `before_send`, cookies) exactly like `send_with`, turns it into a [`StreamOpen`] and opens
//!   it with a [`StreamTransport`]. gRPC messages are encoded by the caller (`rt-grpc`); this
//!   layer moves bytes.
//! - [`StreamTransport`]: the boundary to the network, like [`Transport`](crate::transport::Transport).
//!   Native: own stack (WebSocket and SSE strand A, gRPC over HTTP/2 strand B). Browser: the
//!   companion (`rt agent`, `GET /v1/stream`, strand A) for all three, because the browser
//!   cannot set WebSocket headers or speak gRPC; SSE may also use `fetch` directly.
//! - [`StreamLog`]: the masked history, the result of `rt send` on a session request (JSON)
//!   and what the app shows. Secret values never appear in it.
//!
//! Futures are not `Send` (wasm32).
//!
//! Owner: wave 8 interface. Implementation: strand A ([`open`], WebSocket, SSE, companion,
//! [`StreamLog`]); strand B (gRPC wire, native).

use serde::{Deserialize, Serialize};

/// Wave 8, strand B.
pub mod grpc;
/// The `text/event-stream` parser and a session over a response body (strand A).
pub mod sse;

use std::future::Future as _;
use std::task::{Context, Poll};

use crate::model::{MessageKind, MessageVariant, Protocol, RequestContext, ScriptPhase};
use crate::prepare::{FileSource, PrepareInput, Prepared, prepare};
use crate::script::{Flow, ScriptCall};
use crate::send::{
    SendError, SendOptions, add_secrets, before_wire, declared_secret_values, hook_context, mask,
    mask_transport_error, scope_secrets, script_info, secret_values, set_runtime,
};
use crate::services::{LocalBoxFuture, Note, NoteLevel, Services};
use crate::transport::{HttpRequest, HttpVersion, Transport, TransportError};
use crate::vars::Scope;

/// What to open, after `prepare`. Values are resolved and include secrets: never log it.
/// The companion carries it as its own wire type (`transport::companion`, strand A).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamOpen {
    /// `ws://` or `wss://`; handshake headers (auth, cookies, user headers) in order.
    WebSocket {
        url: String,
        headers: Vec<(String, String)>,
        /// `Sec-WebSocket-Protocol` values, from a user header of that name.
        protocols: Vec<String>,
        /// Connect timeout; `None` for none.
        timeout_ms: Option<u64>,
        /// Ping interval; `None` for none.
        keep_alive_ms: Option<u64>,
    },
    /// An HTTP request whose response is read as `text/event-stream`.
    Sse { request: HttpRequest },
    /// One gRPC call over HTTP/2 (`application/grpc`). `url` is `http://` or `https://`
    /// (from `grpc://`/`grpcs://`), `path` the method path `/package.Service/Method`.
    /// Messages are protobuf bytes without the 5-byte gRPC frame header.
    Grpc {
        url: String,
        path: String,
        /// Metadata (headers), including auth.
        metadata: Vec<(String, String)>,
        timeout_ms: Option<u64>,
    },
}

/// A message on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum Frame {
    Text(String),
    /// Base64 in JSON.
    Binary(#[serde(with = "base64_bytes")] Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Sent,
    Received,
}

/// The response head of a session (handshake for WebSocket, headers for SSE and gRPC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamHead {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: Vec<(String, String)>,
    pub url: String,
}

/// One event of a session. `at_ms`: milliseconds since the session was opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum StreamEvent {
    Opened {
        at_ms: u64,
        head: StreamHead,
    },
    Message {
        at_ms: u64,
        direction: Direction,
        frame: Frame,
        /// SSE: the `event:` field, `id:` field; gRPC: the message type name. `None` otherwise.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// The session ended. WebSocket: close code and reason; gRPC: `grpc-status` as `code`,
    /// `grpc-message` as `reason`, trailers; SSE: the server closed the stream (`code` none).
    Closed {
        at_ms: u64,
        code: Option<u32>,
        reason: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        trailers: Vec<(String, String)>,
        /// Closed by us (`close()`), not by the peer.
        by_client: bool,
    },
    Error {
        at_ms: u64,
        error: TransportError,
    },
}

/// Opens sessions. Implementations: native stack, companion relay, browser fetch (SSE).
pub trait StreamTransport {
    fn open<'a>(
        &'a self,
        open: StreamOpen,
    ) -> LocalBoxFuture<'a, Result<Box<dyn StreamSession + 'a>, TransportError>>;
}

/// An open session. `next` yields the peer's events in order (`Opened` first, `Closed` or
/// `Error` last, then `None`); sent messages are not echoed by the transport, [`Session`]
/// records them.
pub trait StreamSession {
    /// Sends one message. SSE sessions refuse (`TransportError::Other`). For gRPC, client- and
    /// bidi-streaming send any number; unary and server-streaming exactly one.
    fn send(&mut self, frame: Frame) -> LocalBoxFuture<'_, Result<(), TransportError>>;
    /// gRPC: no more messages from the client (half-close). WebSocket and SSE: no-op.
    fn finish(&mut self) -> LocalBoxFuture<'_, Result<(), TransportError>>;
    fn next(&mut self) -> LocalBoxFuture<'_, Option<StreamEvent>>;
    /// Closes the session (WebSocket close 1000, cancels SSE and gRPC).
    fn close(&mut self) -> LocalBoxFuture<'_, ()>;
}

/// The masked history of one session: what `rt send` prints for a session request and what
/// the app shows. Every text (URL, headers, frames, reasons) is masked with the secret values
/// of the scope, like [`SendReport`](crate::send::SendReport).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StreamLog {
    pub version: u32,
    pub request_path: String,
    pub environment: Option<String>,
    /// `websocket`, `sse` or `grpc`.
    pub kind: String,
    /// Masked URL (gRPC: URL and method path).
    pub url: String,
    pub events: Vec<StreamEvent>,
    /// Hints from auth and plugins, as in `SendReport.notes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// An open session with its masking and history; what the shells keep per session id.
pub struct Session<'a> {
    pub inner: Box<dyn StreamSession + 'a>,
    pub log: StreamLog,
    /// Values to mask in everything that goes into `log`.
    pub secrets: Vec<String>,
    pub started: web_time::Instant,
    /// The variables of the session (as prepared), for `{{templates}}` in messages
    /// ([`Session::frame`]).
    pub scope: Scope,
    /// Turns events into what the log shows, before they are masked (wave 8, strand B: gRPC
    /// messages from protobuf to JSON, so secrets inside them are masked as text). `None`
    /// keeps them as they are.
    pub decode: Option<Box<dyn Fn(StreamEvent) -> StreamEvent + 'a>>,
}

/// What a shell asks of a running session ([`Session::drive`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Send(Frame),
    /// gRPC half-close.
    Finish,
    Close,
}

impl Session<'_> {
    /// Sends and records a message (masked in the log). The error is masked too.
    pub async fn send(&mut self, frame: Frame) -> Result<(), TransportError> {
        let at_ms = self.elapsed_ms();
        let recorded = self.decoded(StreamEvent::Message {
            at_ms,
            direction: Direction::Sent,
            frame: frame.clone(),
            name: None,
            id: None,
        });
        let recorded = mask_event(recorded, at_ms, &self.secrets);
        self.inner
            .send(frame)
            .await
            .map_err(|e| mask_transport_error(e, &self.secrets))?;
        self.log.events.push(recorded);
        Ok(())
    }

    /// The next event from the peer, recorded (masked) in the log; the same masked event is
    /// returned. `at_ms` is measured from [`Session::started`]. `None` once the session is
    /// over.
    pub async fn next(&mut self) -> Option<StreamEvent> {
        let event = self.inner.next().await?;
        let event = self.decoded(event);
        let event = mask_event(event, self.elapsed_ms(), &self.secrets);
        self.log.events.push(event.clone());
        Some(event)
    }

    pub async fn close(&mut self) {
        self.inner.close().await;
    }

    fn decoded(&self, event: StreamEvent) -> StreamEvent {
        match &self.decode {
            Some(decode) => decode(event),
            None => event,
        }
    }

    /// The text of a saved message with its `{{variables}}` resolved from the session's
    /// scope. Unknown placeholders stay as written.
    pub fn render(&self, text: &str) -> String {
        self.scope.interpolate(text).value
    }

    /// The frame for a saved message: text, JSON and XML as text, binary from base64 (after
    /// resolving variables). gRPC messages are JSON and are encoded by the caller (`rt-grpc`).
    pub fn frame(&self, message: &MessageVariant) -> Result<Frame, String> {
        message_frame(message, &self.scope)
    }

    /// Runs the session until it ends: hands every event (masked, also the sent messages) to
    /// `on_event` and carries out the commands `poll_command` yields (e.g. a channel's
    /// `poll_recv`). When the command source ends, the session is closed. A failed send is
    /// recorded as an `error` event; the session goes on until the transport ends it.
    pub async fn drive(
        &mut self,
        mut poll_command: impl FnMut(&mut Context<'_>) -> Poll<Option<Command>>,
        mut on_event: impl FnMut(&StreamEvent),
    ) {
        enum Step {
            Event(Option<StreamEvent>),
            Command(Option<Command>),
        }
        let mut commands_open = true;
        loop {
            let step = {
                let mut next = std::pin::pin!(self.next());
                std::future::poll_fn(|cx| {
                    if let Poll::Ready(event) = next.as_mut().poll(cx) {
                        return Poll::Ready(Step::Event(event));
                    }
                    if commands_open && let Poll::Ready(command) = poll_command(cx) {
                        return Poll::Ready(Step::Command(command));
                    }
                    Poll::Pending
                })
                .await
            };
            match step {
                Step::Event(Some(event)) => on_event(&event),
                Step::Event(None) => return,
                Step::Command(Some(Command::Send(frame))) => match self.send(frame).await {
                    Ok(()) => {
                        if let Some(event) = self.log.events.last() {
                            on_event(event);
                        }
                    }
                    Err(error) => self.record_error(error, &mut on_event),
                },
                Step::Command(Some(Command::Finish)) => {
                    if let Err(e) = self.inner.finish().await {
                        let error = mask_transport_error(e, &self.secrets);
                        self.record_error(error, &mut on_event);
                    }
                }
                Step::Command(Some(Command::Close)) => self.close().await,
                Step::Command(None) => {
                    commands_open = false;
                    self.close().await;
                }
            }
        }
    }

    fn record_error(&mut self, error: TransportError, on_event: &mut impl FnMut(&StreamEvent)) {
        let event = StreamEvent::Error {
            at_ms: self.elapsed_ms(),
            error,
        };
        self.log.events.push(event.clone());
        on_event(&event);
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// The frame for a saved message (see [`Session::frame`]).
pub fn message_frame(message: &MessageVariant, scope: &Scope) -> Result<Frame, String> {
    let data = scope.interpolate(&message.data).value;
    match message.kind {
        MessageKind::Text | MessageKind::Json | MessageKind::Xml => Ok(Frame::Text(data)),
        MessageKind::Binary => {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .map(Frame::Binary)
                .map_err(|e| format!("the binary message is not valid base64: {e}"))
        }
    }
}

/// The message a session sends by default: the selected variant, else the first.
pub fn selected_message(messages: &[MessageVariant]) -> Option<&MessageVariant> {
    messages
        .iter()
        .find(|m| m.selected)
        .or_else(|| messages.first())
}

/// Prepares `context` like [`send_with`](crate::send::send_with) and opens it: WebSocket for
/// `Protocol::WebSocket`, gRPC for `Protocol::Grpc` (method path from the spec), SSE for an
/// HTTP request.
///
/// The pipeline is the one of a send: `before-request` scripts (on the request as written;
/// they cannot send requests themselves here), `prepare`, then OAuth 2.0 token, plugin
/// `before_send`, SigV4, client certificate and cookies (`send::before_wire`). `http` is the
/// HTTP transport for what that needs (token requests); `transport` opens the session.
///
/// WebSocket: a user header `Sec-WebSocket-Protocol` becomes the protocol list; the connect
/// timeout is `settings.timeout`, else the request's inherited timeout. SSE: `Accept:
/// text/event-stream` is added unless the request sets `Accept`. `Set-Cookie` of a session's
/// head is not stored, and `after-response` hooks and scripts do not run for sessions.
#[allow(clippy::too_many_arguments)]
pub async fn open<'a, T: Transport, S: StreamTransport + ?Sized>(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    options: &SendOptions,
    services: &Services<'_>,
    http: &T,
    transport: &'a S,
) -> Result<Session<'a>, SendError> {
    let mut notes: Vec<Note> = Vec::new();
    let mut runtime = input.runtime.clone();
    let mut changed = None;
    if let Some(runner) = services.scripts {
        let scripts = crate::script::scripts_for(context, ScriptPhase::BeforeRequest);
        if !scripts.is_empty() {
            let scope = crate::prepare::scope(context, input).unwrap_or_default();
            let mut secrets = declared_secret_values(context, input);
            add_secrets(&mut secrets, scope_secrets(&scope));
            let outcome = runner
                .run(ScriptCall {
                    phase: ScriptPhase::BeforeRequest,
                    scripts,
                    request: crate::script::request_of(&context.request.spec),
                    response: None,
                    variables: crate::script::variables_of(&scope),
                    info: script_info(context, input, options),
                    sender: None,
                })
                .await;
            if let Some(e) = &outcome.error {
                return Err(SendError::Script(mask(e, &secrets)));
            }
            if outcome.flow == Flow::Skip {
                return Err(SendError::Script(
                    "a before-request script skipped the request".into(),
                ));
            }
            for change in &outcome.var_changes {
                set_runtime(&mut runtime, change);
            }
            for log in &outcome.logs {
                notes.push(Note {
                    source: "script:before-request".into(),
                    level: NoteLevel::Info,
                    text: mask(&log.text, &secrets),
                });
            }
            if let Some(request) = &outcome.request {
                let mut entry = context.request.clone();
                crate::script::apply_request(&mut entry.spec, request);
                changed = Some(entry);
            }
        }
    }
    let input = PrepareInput {
        environment: input.environment,
        globals: input.globals.clone(),
        runtime,
    };
    let context = match &changed {
        Some(entry) => RequestContext {
            collection: context.collection,
            folders: context.folders.clone(),
            request: entry,
        },
        None => context.clone(),
    };
    let prepared = prepare(&context, &input, files).map_err(|e| {
        let secrets = declared_secret_values(&context, &input);
        SendError::Prepare(mask(&e.to_string(), &secrets))
    })?;
    let mut secrets = secret_values(&context, &input, &prepared);
    let Prepared {
        request,
        auth,
        scope,
        used,
        ..
    } = prepared;
    let hook_context = hook_context(&context.request.path, &input, &used, &scope);
    let request = before_wire(
        &context,
        &input,
        files,
        request,
        &auth,
        &scope,
        http,
        services,
        &hook_context,
        &mut secrets,
        &mut notes,
    )
    .await?;
    // Wave 10: the distribution's guard sees the final request before the stream opens.
    crate::send::ask_guard(services, &context, &input, &request, &secrets)?;

    let (kind, url, open) = match &context.request.spec.protocol {
        Protocol::WebSocket(spec) => {
            let mut protocols = Vec::new();
            let mut headers = Vec::new();
            for (name, value) in request.headers {
                if name.eq_ignore_ascii_case("sec-websocket-protocol") {
                    protocols.extend(
                        value
                            .split(',')
                            .map(str::trim)
                            .filter(|p| !p.is_empty())
                            .map(str::to_owned),
                    );
                } else {
                    headers.push((name, value));
                }
            }
            let timeout_ms = match spec.timeout_ms {
                Some(0) => None,
                Some(ms) => Some(ms),
                None => request
                    .options
                    .timeout
                    .map(|t| u64::try_from(t.as_millis()).unwrap_or(u64::MAX)),
            };
            let keep_alive_ms = spec.keep_alive_ms.filter(|ms| *ms > 0);
            let url = request.url;
            let open = StreamOpen::WebSocket {
                url: url.clone(),
                headers,
                protocols,
                timeout_ms,
                keep_alive_ms,
            };
            ("websocket", mask(&url, &secrets), open)
        }
        Protocol::Grpc(spec) => {
            let open = grpc::open_for(spec, &request)
                .map_err(|e| SendError::Prepare(mask(&e, &secrets)))?;
            let url = match &open {
                StreamOpen::Grpc { url, path, .. } => format!("{url}{path}"),
                _ => request.url.clone(),
            };
            ("grpc", mask(&url, &secrets), open)
        }
        Protocol::Http => {
            let mut request = request;
            if !request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("accept"))
            {
                request
                    .headers
                    .push(("Accept".into(), "text/event-stream".into()));
            }
            let url = mask(&request.url, &secrets);
            ("sse", url, StreamOpen::Sse { request })
        }
    };
    let log = StreamLog {
        version: LOG_VERSION,
        request_path: context.request.path.clone(),
        environment: input.environment.map(str::to_owned),
        kind: kind.into(),
        url,
        events: Vec::new(),
        notes: notes
            .iter()
            .map(|n| mask(&format!("{}: {}", n.source, n.text), &secrets))
            .collect(),
    };
    let started = web_time::Instant::now();
    let inner = transport
        .open(open)
        .await
        .map_err(|e| SendError::Transport(mask_transport_error(e, &secrets)))?;
    Ok(Session {
        inner,
        log,
        secrets,
        started,
        scope,
        decode: None,
    })
}

/// A readable text of a failed [`open`] (what the shells show).
pub fn error_text(error: &SendError) -> String {
    match error {
        SendError::NotFound(path) => format!("request not found: {path}"),
        SendError::Prepare(m) => m.clone(),
        SendError::Auth(m) => format!("authentication failed: {m}"),
        SendError::Plugin(m) => format!("a plugin stopped the request: {m}"),
        SendError::Script(m) => format!("a before-request script failed: {m}"),
        SendError::Blocked { reason, .. } => format!("not sent: {reason}"),
        SendError::Transport(e) => e.to_string(),
    }
}

/// Version of the [`StreamLog`] JSON shape.
pub const LOG_VERSION: u32 = 1;

/// `frame` with every secret replaced by [`MASK`](crate::send::MASK) (binary: byte-wise).
pub fn mask_frame(frame: &Frame, secrets: &[String]) -> Frame {
    match frame {
        Frame::Text(text) => Frame::Text(mask(text, secrets)),
        Frame::Binary(bytes) => {
            let mut out = bytes.clone();
            for secret in secrets.iter().filter(|s| !s.is_empty()) {
                out = replace_bytes(&out, secret.as_bytes(), crate::send::MASK.as_bytes());
            }
            Frame::Binary(out)
        }
    }
}

fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// `event` masked, with `at_ms` set.
fn mask_event(event: StreamEvent, at_ms: u64, secrets: &[String]) -> StreamEvent {
    let m = |s: &str| mask(s, secrets);
    let pairs = |pairs: Vec<(String, String)>| -> Vec<(String, String)> {
        pairs
            .into_iter()
            .map(|(name, value)| {
                let value = if name.eq_ignore_ascii_case("set-cookie") {
                    crate::cookies::mask_set_cookie(&value)
                } else {
                    value
                };
                let value = m(&value);
                (name, value)
            })
            .collect()
    };
    match event {
        StreamEvent::Opened { head, .. } => StreamEvent::Opened {
            at_ms,
            head: StreamHead {
                status: head.status,
                version: head.version,
                headers: pairs(head.headers),
                url: m(&head.url),
            },
        },
        StreamEvent::Message {
            direction,
            frame,
            name,
            id,
            ..
        } => StreamEvent::Message {
            at_ms,
            direction,
            frame: mask_frame(&frame, secrets),
            name: name.map(|n| m(&n)),
            id: id.map(|i| m(&i)),
        },
        StreamEvent::Closed {
            code,
            reason,
            trailers,
            by_client,
            ..
        } => StreamEvent::Closed {
            at_ms,
            code,
            reason: m(&reason),
            trailers: pairs(trailers),
            by_client,
        },
        StreamEvent::Error { error, .. } => StreamEvent::Error {
            at_ms,
            error: mask_transport_error(error, secrets),
        },
    }
}

/// The HTTP/2 URL of a gRPC request URL: `grpc://h:p` → `http://h:p`, `grpcs://h:p` →
/// `https://h:p`, `host:port` → `http://host:port`; `http(s)://` stay. A path in the URL is
/// kept as a prefix of the method path (Bruno's "subpath" services). Returns the base URL and
/// the full method path.
pub fn grpc_target(url: &str, method: &str) -> Result<(String, String), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("gRPC request without URL".into());
    }
    let (scheme, rest) = match url.split_once("://") {
        Some(("grpc" | "http", rest)) => ("http", rest),
        Some(("grpcs" | "https", rest)) => ("https", rest),
        Some((other, _)) => return Err(format!("unsupported gRPC URL scheme {other}://")),
        None => ("http", url),
    };
    let (authority, prefix) = match rest.split_once('/') {
        Some((authority, path)) => (authority, path.trim_end_matches('/')),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err(format!("gRPC URL without host: {url}"));
    }
    let method = method.trim_start_matches('/');
    let path = if prefix.is_empty() {
        format!("/{method}")
    } else {
        format!("/{prefix}/{method}")
    };
    Ok((format!("{scheme}://{authority}"), path))
}

#[cfg(test)]
mod tests {
    use super::grpc_target;

    #[test]
    fn grpc_urls_map_to_http2() {
        let m = "/hello.HelloService/SayHello";
        assert_eq!(
            grpc_target("grpc://localhost:50051", m).unwrap(),
            ("http://localhost:50051".into(), m.into())
        );
        assert_eq!(
            grpc_target("grpcs://api.example:443/", m).unwrap().0,
            "https://api.example:443"
        );
        assert_eq!(
            grpc_target("localhost:50051", m).unwrap().0,
            "http://localhost:50051"
        );
        assert_eq!(
            grpc_target("grpc://localhost:50051/hello.api", m)
                .unwrap()
                .1,
            "/hello.api/hello.HelloService/SayHello"
        );
        assert!(grpc_target("ws://x", m).is_err());
        assert!(grpc_target("", m).is_err());
    }
}

mod base64_bytes {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&B64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        B64.decode(text).map_err(serde::de::Error::custom)
    }
}
