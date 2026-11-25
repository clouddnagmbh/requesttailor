// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Guided network access for extensions (wave 19, strand B): `Backend.egress`. Extensions
//! have no general network access; plugins have the `network` right, but a built-in or
//! distribution extension (reading a server trace, fetching documentation) goes through
//! here. The request uses the app's own `Transport` and these rules, all enforced in Rust:
//!
//! - only hosts the user allowed **once per device** ([`EgressHost`]; the UI asks, the list
//!   lives in `egress/hosts.json` in the data folder, in the browser's storage on the web);
//!   anything else is [`EgressError::NotAllowed`];
//! - `{{secret:<name>}}` in a header is resolved in Rust and masked in every output;
//! - no redirects (a 3xx comes back as it is), at most [`MAX_RESPONSE`] bytes of response,
//!   [`TIMEOUT_MS`] ms, only `GET` and `POST` with a body up to [`MAX_BODY`] bytes;
//! - nothing goes to the history or to the response memory;
//! - **every call goes through the guard** with `Purpose::Egress`; a guard that refuses (for
//!   instance an air-gap policy) wins ([`EgressError::Blocked`]);
//! - on the web a browser that refuses with CORS goes through the companion service.
//!
//! [`request`] is the one function every shell calls (native, browser and companion path
//! differ only in the transport they pass); [`HostList`] is the list of allowed hosts.

mod call;
mod hosts;

pub use call::{Context, lookup_secrets, request, secret_names};
#[cfg(not(target_arch = "wasm32"))]
pub use hosts::file;
pub use hosts::{HostError, HostList, MAX_HOSTS};

use serde::{Deserialize, Serialize};

/// Largest response body kept (4 MiB); the rest is cut and `truncated` is set.
pub const MAX_RESPONSE: usize = 4 * 1024 * 1024;
/// Largest request body (256 KiB).
pub const MAX_BODY: usize = 256 * 1024;
/// Time for one call (30 s).
pub const TIMEOUT_MS: u64 = 30_000;

/// The method of an egress request; nothing else is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EgressMethod {
    Get,
    Post,
}

/// One call (`EgressApi.request`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressRequest {
    pub method: EgressMethod,
    /// An `http` or `https` URL; its host (and port) must be allowed.
    pub url: String,
    /// Header values may carry `{{secret:<name>}}`, resolved in Rust.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Who asks and why, shown in the one-time question and in the list of hosts: the
    /// extension's id and a short purpose such as "Server traces".
    pub extension: String,
    pub purpose: String,
    /// The collection whose secrets `{{secret:<name>}}` names (`None`: the active one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    /// The environment whose secret variable `{{secret:<name>}}` names (`None`: the first
    /// environment of the collection that has a value for it). Additive (wave 19, strand B).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}

/// The answer. Header values and the body are masked (every secret value of the call).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// The body as text (lossy UTF-8), at most [`MAX_RESPONSE`] bytes.
    pub body: String,
    /// The body was cut at [`MAX_RESPONSE`].
    #[serde(default)]
    pub truncated: bool,
    pub took_ms: u64,
}

/// A host the user allowed on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressHost {
    pub host: String,
    /// `None`: port 443 (the default of `https`); a host reached over plain `http` on its
    /// default port is listed with `Some(80)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The extension and purpose named when it was allowed.
    pub extension: String,
    pub purpose: String,
    /// Unix milliseconds.
    pub allowed_at: i64,
}

/// Why a call did not happen or failed. Strand B maps it to an `AppError` the shells pass on
/// (the UI tells `not-allowed` apart to ask the user once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EgressError {
    /// The host is not on the device's list; the UI may ask the user and call `allow`.
    NotAllowed { host: String, port: Option<u16> },
    /// The guard refused (`rule`, `reason` as in `rt_core::guard`).
    Blocked { rule: String, reason: String },
    /// A body or response over its limit, or a method other than `GET`/`POST`.
    Limit { message: String },
    /// Timeout, connection or TLS failure; the message is masked.
    Transport { message: String },
}

impl std::fmt::Display for EgressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EgressError::NotAllowed { host, .. } => write!(f, "{host} is not an allowed host"),
            EgressError::Blocked { reason, .. } => write!(f, "blocked: {reason}"),
            EgressError::Limit { message } | EgressError::Transport { message } => {
                f.write_str(message)
            }
        }
    }
}

impl std::error::Error for EgressError {}
