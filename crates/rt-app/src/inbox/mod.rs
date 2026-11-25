// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The local webhook inbox (wave 18, strand E): a loopback listener (default port
//! [`DEFAULT_PORT`]) that takes requests of any method on any path, answers with a fixed
//! reply and shows what arrived. It starts only on request, binds to loopback only, sends
//! no CORS headers, keeps everything in memory (no file, history, exchange or MCP tool) and
//! has hard limits ([`MAX_BODY_BYTES`], [`MAX_PER_SECOND`], [`MAX_ENTRIES`],
//! [`MAX_TOTAL_BYTES`]). No tunnel, no relay.
//!
//! The data types are `rt_core::inbox` (the companion protocol carries them to the browser);
//! the listener lives in `server.rs` (strand E, native only); turning a capture into a
//! request ([`InboxDraft`]) runs everywhere. Making a request: `Authorization`, `Cookie`
//! and headers whose name contains `signature`, `token`, `secret` or `key` become secret
//! variables **without a value**; the secret scanner runs over headers and body.
//!
//! `buffer` (the bounded store) and `draft` run everywhere; `server` (the listener, [`Inbox`])
//! is native only. The browser reaches an inbox through the companion (`rt agent`, route
//! `/v1/inbox`).

use serde::{Deserialize, Serialize};

use rt_core::model::RequestSpec;

mod buffer;
mod draft;
#[cfg(not(target_arch = "wasm32"))]
mod server;

pub use buffer::{Buffer, Limits};
pub use draft::{BASE_URL, draft};
#[cfg(not(target_arch = "wasm32"))]
pub use server::{Inbox, is_loopback_peer, validate};

pub use rt_core::inbox::{
    CapturedRequest, DEFAULT_PORT, InboxEvent, InboxPage, InboxSettings, InboxStatus,
    MAX_BODY_BYTES, MAX_DELAY_MS, MAX_ENTRIES, MAX_PER_SECOND, MAX_TOTAL_BYTES,
};

/// A captured request made into a request that replays it against `{{baseUrl}}`
/// (`InboxApi.makeRequest`), unsaved until the user confirms it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxDraft {
    pub name: String,
    pub spec: RequestSpec,
    /// Names of the secret variables the draft needs (the user sets their values).
    pub secrets: Vec<String>,
}
