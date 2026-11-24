// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The data of the local webhook inbox (wave 18, strand E). The listener itself is
//! `rt_app::inbox` (native only); the types live here because the companion protocol
//! (`transport::companion`, route `/v1/inbox`) carries them to the browser. `rt_app::inbox`
//! re-exports them.
//!
//! The inbox listens on loopback only, answers with a fixed reply, holds what it captured in
//! memory only (never a file, a history, an exchange or an MCP tool) and has hard limits.

use serde::{Deserialize, Serialize};

/// The default port of the inbox (loopback). `0` in [`InboxSettings::port`] picks a free one.
pub const DEFAULT_PORT: u16 = 47855;
/// Largest request body taken (2 MiB); more is answered with `413`.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Most requests taken per second; more is answered with `429`.
pub const MAX_PER_SECOND: u32 = 200;
/// Most captured requests kept; the oldest falls out and `dropped` counts it.
pub const MAX_ENTRIES: usize = 500;
/// Most bytes kept over all captured requests (64 MiB).
pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Longest delay before the fixed answer (milliseconds).
pub const MAX_DELAY_MS: u32 = 5_000;

/// What the inbox answers. The default is `200` without a body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxSettings {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Up to [`MAX_DELAY_MS`].
    #[serde(default)]
    pub delay_ms: u32,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_status() -> u16 {
    200
}

impl Default for InboxSettings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            status: 200,
            body: String::new(),
            content_type: None,
            delay_ms: 0,
        }
    }
}

/// One request the inbox took.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedRequest {
    /// Counts from 1 since the inbox started.
    pub id: u64,
    /// Unix milliseconds.
    pub at: i64,
    pub method: String,
    /// Path and query as received, e.g. `/hooks/orders?x=1`.
    pub target: String,
    pub headers: Vec<(String, String)>,
    /// The body as text when it is UTF-8; `None` for binary bodies.
    pub body: Option<String>,
    /// Size of the body in bytes.
    pub size: u64,
}

/// What the inbox tells a subscriber.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InboxEvent {
    Started {
        port: u16,
    },
    Stopped,
    Request {
        request: Box<CapturedRequest>,
    },
    /// `count` older requests fell out of the buffer.
    Dropped {
        count: u64,
    },
    Cleared,
}

/// The state of the inbox.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InboxStatus {
    pub running: bool,
    /// The port in use while running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// `http://127.0.0.1:<port>/` while running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Requests held now.
    pub held: u64,
    /// Requests that fell out of the buffer since it started.
    pub dropped: u64,
}

/// Events after `since`, for subscribers that poll (every shell can; the desktop and the
/// companion may also push).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InboxPage {
    pub events: Vec<InboxEvent>,
    /// Ask again with this as `since`.
    pub next: u64,
}
