// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One entry point for "send this request": CLI, WASM and later the app all call this, so
//! their results are identical (product principle 5). `SendReport` is also the JSON that
//! `rt send` prints and that `rt-wasm` returns.
//!
//! Owner: wave 2, strand D.

use serde::{Deserialize, Serialize};

use crate::model::Collection;
use crate::prepare::{FileSource, PrepareInput};
use crate::transport::{HttpVersion, Redirect, Timing, Transport, TransportError};
use crate::vars::VarUse;

/// Version of the `SendReport` JSON shape. Bump on breaking changes.
pub const REPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendOptions {
    /// Keep at most this many body bytes in the report.
    pub body_limit: usize,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            body_limit: 10 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentRequest {
    pub method: String,
    pub url: String,
    /// Secret values are masked as `***`.
    pub headers: Vec<(String, String)>,
    pub body_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding", content = "data", rename_all = "lowercase")]
pub enum ReportBody {
    /// Valid UTF-8.
    Utf8(String),
    /// Anything else, standard base64.
    Base64(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedResponse {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub redirects: Vec<Redirect>,
    pub body: ReportBody,
    pub size: u64,
    pub truncated: bool,
    pub timing: Timing,
    #[serde(with = "crate::serde_ms")]
    pub total: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum SendError {
    /// Request path not in the collection.
    NotFound(String),
    /// Variable, auth or body preparation failed.
    Prepare(String),
    Transport(TransportError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendReport {
    pub version: u32,
    /// Request path relative to the collection root.
    pub request_path: String,
    pub environment: Option<String>,
    /// What went on the wire; `None` if preparation failed.
    pub request: Option<SentRequest>,
    pub response: Option<ReceivedResponse>,
    pub variables: Vec<VarUse>,
    pub unresolved: Vec<String>,
    pub error: Option<SendError>,
}

pub async fn send<T: Transport>(
    collection: &Collection,
    request_path: &str,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    transport: &T,
    options: &SendOptions,
) -> SendReport {
    let _ = (collection, request_path, input, files, transport, options);
    todo!("wave 2, strand D")
}
