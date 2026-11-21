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
//!
//! The pairing flow and the origin/host checks are owned by the companion (wave 3, strand G)
//! and documented in `crates/rt-cli/README.md`.
//!
//! Owner: wave 3, strand G. Changes to these types are interface changes.

use serde::{Deserialize, Serialize};

use super::{HttpRequest, HttpVersion, Redirect, ResponseHead, Timing};

/// Protocol version spoken by this build.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub protocol: u32,
    /// `rt` version, e.g. `0.1.0`.
    pub version: String,
    /// Whether the request's `Origin` is already paired (its token is still valid).
    pub paired: bool,
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

impl From<&HttpRequest> for WireRequest {
    fn from(request: &HttpRequest) -> Self {
        let _ = request;
        todo!("wave 3, strand G")
    }
}

impl TryFrom<WireRequest> for HttpRequest {
    type Error = String;
    fn try_from(wire: WireRequest) -> Result<Self, String> {
        let _ = wire;
        todo!("wave 3, strand G")
    }
}

impl From<&ResponseHead> for WireHead {
    fn from(head: &ResponseHead) -> Self {
        let _ = head;
        todo!("wave 3, strand G")
    }
}

impl From<WireHead> for ResponseHead {
    fn from(wire: WireHead) -> Self {
        let _ = wire;
        todo!("wave 3, strand G")
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

#[cfg(target_arch = "wasm32")]
impl super::Transport for CompanionTransport {
    async fn send(&self, request: HttpRequest) -> Result<super::Response, super::TransportError> {
        let _ = request;
        todo!("wave 3, strand G")
    }
}
