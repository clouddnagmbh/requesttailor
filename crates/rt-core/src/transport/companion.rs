// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The companion protocol: the web app hands a prepared [`HttpRequest`] to `rt agent` on the
//! local machine, which sends it with the native transport. Everything above the transport
//! (variables, auth, body, report) still runs in the browser's WASM core, so results match.
//!
//! # Wire protocol (version 1)
//!
//! All endpoints on the companion's loopback address (`http://127.0.0.1:<port>`):
//!
//! - `GET /v1/status` → `200` JSON [`Status`]. No token needed.
//! - `POST /v1/pair` with JSON [`PairRequest`] → `200` JSON [`PairResponse`], or `403`.
//! - `POST /v1/send` with `Authorization: Bearer <token>` and JSON [`WireRequest`] →
//!   - success: `200`, header `x-rt-head` = JSON [`WireHead`], body = the raw response body,
//!     streamed;
//!   - transport failure: `502`, JSON [`TransportError`];
//!   - bad or missing token: `401`.
//! - `POST /v1/secrets` with `Authorization: Bearer <token>` and JSON [`SecretsRequest`]
//!   (wave 5): the companion reads the external secrets with the secret managers' CLIs on
//!   this machine (`rt_app::secret_sources`) →
//!   - success: `200` JSON [`SecretsResponse`];
//!   - a secret could not be read (CLI missing, not signed in, timeout): `422` JSON
//!     [`SecretsError`];
//!   - the companion was started without `--allow-secrets`: `403` JSON [`SecretsError`];
//!   - bad or missing token: `401`.
//!
//! - `GET /v1/stream?token=<token>` (wave 8): a WebSocket upgrade on loopback (browsers
//!   cannot set headers on a WebSocket, so the token is in the query; the origin check still
//!   applies). The browser's session runs over it as JSON text messages: first a
//!   [`StreamClientMessage::Open`], then `Send`/`Finish`/`Close`; the companion answers
//!   with [`StreamServerMessage`]s (each a [`StreamEvent`](crate::stream::StreamEvent) of the
//!   native session, or `Failed`). One session per WebSocket. `Status.features` lists
//!   `"stream"` when the companion supports it.
//!
//! - `GET /v1/inbox?token=<token>` (wave 18): a WebSocket upgrade on loopback like
//!   `/v1/stream`, for the webhook inbox (`crate::inbox`): the browser sends an
//!   [`InboxClientMessage`], the companion answers with [`InboxServerMessage`]s. The inbox
//!   itself listens on loopback only and keeps its requests in memory. `Status.features`
//!   lists `"inbox"` when the companion supports it.
//!
//! The pairing flow and the origin/host checks are owned by the companion (wave 3, strand G)
//! and documented in `crates/rt-cli/README.md`.
//!
//! Owner: wave 3, strand G. Changes to these types are interface changes.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

use super::{HttpBody, HttpRequest, HttpVersion, Redirect, ResponseHead, SendOptions, Timing};
use crate::model::{ExternalSecret, SecretProvider};

/// Protocol version spoken by this build.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub protocol: u32,
    /// `rt` version, e.g. `0.1.0`.
    pub version: String,
    /// Whether the request's `Origin` is already paired (its token is still valid).
    pub paired: bool,
    /// Optional endpoints this companion has (wave 8: `"stream"`). Older companions send none.
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    /// One-time code shown by `rt agent` (or carried in its pairing link).
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairResponse {
    pub token: String,
}

/// [`HttpRequest`] on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Body bytes, standard base64.
    pub body: Option<String>,
    pub content_type: Option<String>,
    pub timeout_ms: Option<u64>,
    pub follow_redirects: bool,
    pub max_redirects: u32,
}

/// Body of `POST /v1/secrets`: the references of one environment's external secrets (never
/// values). Only enabled entries are read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsRequest {
    pub provider: SecretProvider,
    pub variables: Vec<ExternalSecret>,
}

/// Answer of `POST /v1/secrets`: `(variable, value)` for each enabled entry of the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsResponse {
    pub values: Vec<(String, String)>,
}

/// Error answer of `POST /v1/secrets` (`403`, `422`): what went wrong and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsError {
    pub error: String,
}

/// [`ResponseHead`] on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireHead {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub redirects: Vec<Redirect>,
    pub timing: Timing,
}

/// Response header of a successful `/v1/send` that carries the [`WireHead`] as JSON.
/// The companion lists it in `Access-Control-Expose-Headers` so `fetch` can read it.
pub const HEAD_HEADER: &str = "x-rt-head";

/// A request header of `POST /v1/send` that marks the call as an *egress* call of an extension
/// (wave 19, `rt_app::egress`). The companion removes it before the request goes out and does
/// not hand the exchange to its observers: an egress call leaves no trace in a distribution's
/// memory of exchanges.
pub const EGRESS_HEADER: &str = "x-rt-egress";

/// Encodes a [`WireHead`] for [`HEAD_HEADER`]. Everything but printable ASCII is escaped as
/// `\uXXXX`: browsers decode header values as Latin-1, and a header value may not contain
/// control characters.
pub fn encode_head(head: &WireHead) -> String {
    let json = serde_json::to_string(head).expect("WireHead serializes");
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if (' '..='~').contains(&c) {
            out.push(c);
        } else {
            // Only occurs inside JSON strings (serde_json escapes the other control
            // characters already), where `\u` escapes are valid.
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// Decodes the value of [`HEAD_HEADER`].
pub fn decode_head(value: &str) -> Result<WireHead, String> {
    serde_json::from_str(value).map_err(|e| format!("invalid {HEAD_HEADER} header: {e}"))
}

impl From<&HttpRequest> for WireRequest {
    fn from(request: &HttpRequest) -> Self {
        Self {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: request.headers.clone(),
            body: request.body.as_ref().map(|b| B64.encode(&b.bytes)),
            content_type: request.body.as_ref().and_then(|b| b.content_type.clone()),
            timeout_ms: request
                .options
                .timeout
                .map(|t| u64::try_from(t.as_millis()).unwrap_or(u64::MAX)),
            follow_redirects: request.options.follow_redirects,
            max_redirects: request.options.max_redirects,
        }
    }
}

impl TryFrom<WireRequest> for HttpRequest {
    type Error = String;
    fn try_from(wire: WireRequest) -> Result<Self, String> {
        let body = match (wire.body, wire.content_type) {
            (Some(data), content_type) => Some(HttpBody {
                content_type,
                bytes: B64
                    .decode(data.as_bytes())
                    .map_err(|e| format!("body is not valid base64: {e}"))?
                    .into(),
            }),
            (None, Some(_)) => return Err("content_type without body".to_owned()),
            (None, None) => None,
        };
        Ok(Self {
            method: wire.method,
            url: wire.url,
            headers: wire.headers,
            body,
            options: SendOptions {
                client_identity: None,
                timeout: wire.timeout_ms.map(Duration::from_millis),
                follow_redirects: wire.follow_redirects,
                max_redirects: wire.max_redirects,
            },
        })
    }
}

impl From<&ResponseHead> for WireHead {
    fn from(head: &ResponseHead) -> Self {
        Self {
            status: head.status,
            version: head.version,
            headers: head.headers.clone(),
            url: head.url.clone(),
            redirects: head.redirects.clone(),
            timing: head.timing.clone(),
        }
    }
}

impl From<WireHead> for ResponseHead {
    fn from(wire: WireHead) -> Self {
        Self {
            status: wire.status,
            version: wire.version,
            headers: wire.headers,
            url: wire.url,
            redirects: wire.redirects,
            timing: wire.timing,
        }
    }
}

/// Browser side: sends through a paired companion via `fetch`. Implements [`super::Transport`]
/// on wasm32; on other targets only the types above are used (by `rt agent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionTransport {
    /// e.g. `http://127.0.0.1:47812`
    pub base_url: String,
    pub token: String,
}

impl CompanionTransport {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
        }
    }
}

/// The browser's own `fetch` timeout is the request's timeout plus this, so that the
/// companion's more precise timeout error normally arrives first.
#[cfg(any(target_arch = "wasm32", test))]
const FETCH_GRACE: Duration = Duration::from_secs(2);

/// Error bodies (`502`) larger than this are not read.
#[cfg(any(target_arch = "wasm32", test))]
const ERROR_BODY_LIMIT: usize = 64 * 1024;

#[cfg(any(target_arch = "wasm32", test))]
impl CompanionTransport {
    /// The `fetch` call to the companion that carries `request`.
    fn envelope(&self, request: &HttpRequest) -> Result<HttpRequest, super::TransportError> {
        // Wave 5: the companion protocol carries no key material; never send without the
        // certificate the request needs.
        if request.options.client_identity.is_some() {
            return Err(super::TransportError::Tls(
                "client certificates (mTLS) are not sent through the companion; send this \
                 request from the desktop app or the CLI"
                    .to_owned(),
            ));
        }
        let json = serde_json::to_vec(&WireRequest::from(request))
            .map_err(|e| super::TransportError::Other(e.to_string()))?;
        Ok(HttpRequest {
            method: "POST".to_owned(),
            url: format!("{}/v1/send", self.base_url.trim_end_matches('/')),
            headers: vec![("Authorization".to_owned(), format!("Bearer {}", self.token))],
            body: Some(HttpBody {
                content_type: Some("application/json".to_owned()),
                bytes: json.into(),
            }),
            options: SendOptions {
                client_identity: None,
                timeout: request.options.timeout.map(|t| t + FETCH_GRACE),
                follow_redirects: false,
                max_redirects: 0,
            },
        })
    }

    /// Turns the companion's answer into the upstream response (or its transport error).
    async fn unwrap_response(
        &self,
        response: super::Response,
        timeout: Option<Duration>,
        started: web_time::Instant,
    ) -> Result<super::Response, super::TransportError> {
        use super::TransportError;
        match response.head.status {
            200 => {
                let value = response
                    .head
                    .headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(HEAD_HEADER))
                    .map(|(_, value)| value.as_str())
                    .ok_or_else(|| {
                        TransportError::Other(format!("the companion sent no {HEAD_HEADER} header"))
                    })?;
                let head = decode_head(value).map_err(TransportError::Other)?;
                Ok(super::Response {
                    head: head.into(),
                    body: Box::pin(CompanionBody {
                        inner: response.body,
                        timeout,
                        started,
                    }),
                })
            }
            502 => {
                let body = super::collect(response, ERROR_BODY_LIMIT, started)
                    .await?
                    .body;
                Err(
                    serde_json::from_slice::<TransportError>(&body).unwrap_or_else(|_| {
                        TransportError::Other(format!(
                            "the companion failed: {}",
                            String::from_utf8_lossy(&body)
                        ))
                    }),
                )
            }
            // Blocked, so the UI offers the companion again and can pair anew.
            401 => Err(TransportError::Blocked(
                "the companion no longer accepts this browser's pairing; pair again".to_owned(),
            )),
            status => Err(TransportError::Other(format!(
                "the companion answered HTTP {status}"
            ))),
        }
    }
}

/// Body of the upstream response, read from the companion's response. A broken-off
/// connection is reported as a timeout once the request's timeout has passed (the companion
/// cuts the connection when its timeout fires during the body).
#[cfg(any(target_arch = "wasm32", test))]
struct CompanionBody {
    inner: super::BodyStream,
    timeout: Option<Duration>,
    started: web_time::Instant,
}

#[cfg(any(target_arch = "wasm32", test))]
impl futures_core::Stream for CompanionBody {
    type Item = Result<bytes::Bytes, super::TransportError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use super::TransportError;
        let this = self.get_mut();
        let (timeout, started) = (this.timeout, this.started);
        this.inner.as_mut().poll_next(cx).map(|item| {
            item.map(|chunk| {
                chunk.map_err(|e| match e {
                    TransportError::Timeout => TransportError::Timeout,
                    _ if timeout.is_some_and(|t| started.elapsed() >= t) => TransportError::Timeout,
                    e => TransportError::Other(format!(
                        "the connection to the companion broke off: {e}"
                    )),
                })
            })
        })
    }
}

#[cfg(target_arch = "wasm32")]
impl super::Transport for CompanionTransport {
    async fn send(&self, request: HttpRequest) -> Result<super::Response, super::TransportError> {
        use super::TransportError;
        let envelope = self.envelope(&request)?;
        let started = web_time::Instant::now();
        let response = super::Transport::send(&super::fetch::FetchTransport::new(), envelope)
            .await
            .map_err(|e| match e {
                // Blocked, so the UI offers the companion again (and then says how to start it).
                TransportError::Blocked(m) => TransportError::Blocked(format!(
                    "the companion at {} is not reachable ({m})",
                    self.base_url
                )),
                e => e,
            })?;
        self.unwrap_response(response, request.options.timeout, started)
            .await
    }
}

#[cfg(test)]
mod tests;

/// [`StreamOpen`](crate::stream::StreamOpen) on the wire (wave 8, `GET /v1/stream`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WireStreamOpen {
    #[serde(rename = "websocket")]
    WebSocket {
        url: String,
        headers: Vec<(String, String)>,
        protocols: Vec<String>,
        timeout_ms: Option<u64>,
        keep_alive_ms: Option<u64>,
    },
    Sse {
        request: WireRequest,
    },
    Grpc {
        url: String,
        path: String,
        metadata: Vec<(String, String)>,
        timeout_ms: Option<u64>,
    },
}

/// Browser → companion on `/v1/stream`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum StreamClientMessage {
    Open { open: WireStreamOpen },
    Send { frame: crate::stream::Frame },
    Finish,
    Close,
}

/// Companion → browser on `/v1/stream`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum StreamServerMessage {
    /// An event of the native session.
    Event { event: crate::stream::StreamEvent },
    /// Opening or sending failed; the session is over.
    Failed { error: super::TransportError },
}

/// Browser → companion on `/v1/inbox` (wave 18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum InboxClientMessage {
    /// Starts the inbox (it answers with `Status`, or `Failed` if the port is taken).
    Start {
        settings: crate::inbox::InboxSettings,
    },
    Stop,
    Clear,
    /// Asks for the current status.
    Status,
}

/// Companion → browser on `/v1/inbox` (wave 18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum InboxServerMessage {
    Status { status: crate::inbox::InboxStatus },
    Event { event: crate::inbox::InboxEvent },
    Failed { message: String },
}

impl From<crate::stream::StreamOpen> for WireStreamOpen {
    fn from(open: crate::stream::StreamOpen) -> Self {
        use crate::stream::StreamOpen;
        match open {
            StreamOpen::WebSocket {
                url,
                headers,
                protocols,
                timeout_ms,
                keep_alive_ms,
            } => WireStreamOpen::WebSocket {
                url,
                headers,
                protocols,
                timeout_ms,
                keep_alive_ms,
            },
            StreamOpen::Sse { request } => WireStreamOpen::Sse {
                request: WireRequest::from(&request),
            },
            StreamOpen::Grpc {
                url,
                path,
                metadata,
                timeout_ms,
            } => WireStreamOpen::Grpc {
                url,
                path,
                metadata,
                timeout_ms,
            },
        }
    }
}
