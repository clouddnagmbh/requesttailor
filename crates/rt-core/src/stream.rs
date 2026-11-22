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

use crate::model::RequestContext;
use crate::send::{SendError, SendOptions};
use crate::services::{LocalBoxFuture, Services};
use crate::transport::{HttpRequest, HttpVersion, TransportError};

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
}

impl Session<'_> {
    /// Sends and records a message (masked in the log). Strand A.
    pub async fn send(&mut self, _frame: Frame) -> Result<(), TransportError> {
        Err(TransportError::Other(
            "sessions arrive in wave 8, strand A".into(),
        ))
    }

    /// The next event from the peer, recorded (masked) in the log. Strand A.
    pub async fn next(&mut self) -> Option<StreamEvent> {
        None
    }

    pub async fn close(&mut self) {
        self.inner.close().await;
    }
}

/// Prepares `context` like `send_with` and opens it: WebSocket for `Protocol::WebSocket`,
/// gRPC for `Protocol::Grpc` (method path from the spec), SSE for an HTTP request. Strand A
/// (gRPC: the `StreamOpen` mapping is strand B's, in `stream/grpc.rs`).
pub async fn open<'a, S: StreamTransport + ?Sized>(
    _context: &RequestContext<'_>,
    _options: &SendOptions,
    _services: &Services<'_>,
    _transport: &'a S,
) -> Result<Session<'a>, SendError> {
    Err(SendError::Prepare(
        "sessions (WebSocket, SSE, gRPC) arrive in wave 8".into(),
    ))
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
