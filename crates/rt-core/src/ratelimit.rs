// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Rate limits (wave 18, strand D): reading `RateLimit-*`, `X-RateLimit-*` and `Retry-After`,
//! and the runner's optional throttling (`RunPlan.throttle`). The types are the interface;
//! `parse` and `observe` answer "nothing" until strand D, the tracker waits for nobody.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::send::ReceivedResponse;

/// What a response said about the limits of its host.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RateLimits {
    /// Requests allowed per window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Requests left in the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<u64>,
    /// Milliseconds until the window resets (relative to the response).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_ms: Option<u64>,
    /// Milliseconds the server asks to wait (`Retry-After`, after `429` or `503`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// Windows named by `RateLimit-Policy`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<RateWindow>,
}

/// One quota window, e.g. `100;w=60`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateWindow {
    pub limit: u64,
    /// Window length in seconds.
    pub seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThrottleMode {
    /// The default: the runner never waits for a rate limit.
    #[default]
    Off,
    /// Wait for the reset or `Retry-After` (up to `max_wait_ms`) and retry.
    Respect,
}

/// The runner's behaviour towards rate limits (`rt run --respect-limits [--max-wait 60s]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Throttle {
    #[serde(default)]
    pub mode: ThrottleMode,
    /// Longest single wait; default 60 s, hard maximum [`MAX_WAIT_HARD_MS`].
    #[serde(default = "default_max_wait")]
    pub max_wait_ms: u64,
    /// Retries per request (reads only; writes are never retried automatically).
    #[serde(default = "default_retries")]
    pub retries: u32,
}

pub const DEFAULT_MAX_WAIT_MS: u64 = 60_000;
pub const MAX_WAIT_HARD_MS: u64 = 600_000;
pub const DEFAULT_RETRIES: u32 = 2;

fn default_max_wait() -> u64 {
    DEFAULT_MAX_WAIT_MS
}

fn default_retries() -> u32 {
    DEFAULT_RETRIES
}

impl Default for Throttle {
    fn default() -> Self {
        Self {
            mode: ThrottleMode::Off,
            max_wait_ms: DEFAULT_MAX_WAIT_MS,
            retries: DEFAULT_RETRIES,
        }
    }
}

impl Throttle {
    /// Serde helper: leave the default out of `RunPlan` JSON.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Per-host state of the limits seen so far. The page follower and the monitors share it
/// with the runner. Interface: an empty store until strand D.
#[derive(Debug, Clone, Default)]
pub struct RateTracker {
    hosts: BTreeMap<String, RateLimits>,
}

impl RateTracker {
    /// What is known about `host`.
    pub fn get(&self, host: &str) -> Option<&RateLimits> {
        self.hosts.get(host)
    }
}

/// Reads the limits a response states; `None` when it states none. Interface: always `None`
/// until strand D.
pub fn parse(headers: &[(String, String)]) -> Option<RateLimits> {
    let _ = headers;
    None
}

/// Called by `send_with` for every response; the result becomes
/// [`crate::send::SendReport::rate_limit`]. Interface: always `None` until strand D.
pub fn observe(response: &ReceivedResponse) -> Option<RateLimits> {
    parse(&response.headers)
}
