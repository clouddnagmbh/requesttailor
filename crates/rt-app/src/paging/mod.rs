// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Following pages and merging them (wave 18, strand C). A rule says how the next page is
//! found ([`PagingRule`]: `link-header`, `json-url`, `cursor`); it comes from the request's
//! companion `<request>.paging.json`, the collection default
//! (`extensions.requesttailor.paging`) or a plugin (`contributes.paging`), in that order of
//! precedence. The core knows no vendor.
//!
//! Limits are mandatory ([`PagingLimits`], hard maxima that no setting lifts). Only reads
//! follow, only to the origin of the request; every page goes through `send_with` (prepare,
//! guard, plugin hooks, rate limits). Interface: types only.

use serde::{Deserialize, Serialize};

pub use rt_core::paging::{CursorParam, CursorPlace, PagingRule, PagingSpec};

use crate::{SendInput, StoredResponse};

pub const DEFAULT_MAX_PAGES: u32 = 20;
pub const HARD_MAX_PAGES: u32 = 1_000;
pub const DEFAULT_MAX_BYTES: u64 = 50 * 1024 * 1024;
pub const HARD_MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const DEFAULT_MAX_SECONDS: u64 = 120;
pub const HARD_MAX_SECONDS: u64 = 600;

/// How far one run goes. Values above the hard maxima are cut to them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagingLimits {
    #[serde(default = "pages")]
    pub max_pages: u32,
    #[serde(default = "bytes")]
    pub max_bytes: u64,
    #[serde(default = "seconds")]
    pub max_seconds: u64,
}

fn pages() -> u32 {
    DEFAULT_MAX_PAGES
}
fn bytes() -> u64 {
    DEFAULT_MAX_BYTES
}
fn seconds() -> u64 {
    DEFAULT_MAX_SECONDS
}

impl Default for PagingLimits {
    fn default() -> Self {
        Self {
            max_pages: DEFAULT_MAX_PAGES,
            max_bytes: DEFAULT_MAX_BYTES,
            max_seconds: DEFAULT_MAX_SECONDS,
        }
    }
}

impl PagingLimits {
    /// The limits cut to the hard maxima (and at least one page, one byte, one second).
    pub fn clamped(&self) -> Self {
        Self {
            max_pages: self.max_pages.clamp(1, HARD_MAX_PAGES),
            max_bytes: self.max_bytes.clamp(1, HARD_MAX_BYTES),
            max_seconds: self.max_seconds.clamp(1, HARD_MAX_SECONDS),
        }
    }
}

/// Where the rule in force comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PagingOrigin {
    Request,
    Collection,
    Plugin { plugin: String, id: String },
}

/// The rule that applies to a request, with where it comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectivePaging {
    pub spec: PagingSpec,
    pub origin: PagingOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PagingMode {
    /// One more page after `response_id`.
    Next,
    /// Every page, up to the limits.
    All,
}

/// `PagingApi.run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagingRun {
    pub path: String,
    pub input: SendInput,
    pub mode: PagingMode,
    /// The page to continue from (`Next` only).
    #[serde(default)]
    pub response_id: Option<u64>,
    #[serde(default)]
    pub limits: Option<PagingLimits>,
}

/// Why a run stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum PagingStop {
    /// No next page.
    Done,
    MaxPages,
    MaxBytes,
    MaxSeconds,
    /// The same cursor or URL came twice.
    Loop,
    /// The next URL has another origin; `message` names it.
    OtherOrigin(String),
    /// A page answered with an error status.
    ErrorStatus(u16),
    /// The guard or a plugin refused a page.
    Refused(String),
    /// The request writes; only reads follow.
    NotRead,
}

/// What a run made: one report with the merged body (kept on the Rust side like any
/// response), and why it stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PagedResult {
    pub response: StoredResponse,
    pub pages: u32,
    pub stopped: PagingStop,
    /// More pages exist (the run stopped before the end).
    pub more: bool,
}

/// Where `PagingApi::set_rule` saves a rule: in the request file, or as the default of the
/// collection (`extensions.requesttailor.paging` of `opencollection.yml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PagingRuleScope {
    Request,
    Collection,
}
