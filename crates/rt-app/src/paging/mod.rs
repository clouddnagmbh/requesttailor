// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Following pages and merging them (wave 18, strand C). A rule says how the next page is
//! found ([`PagingRule`]: `link-header`, `json-url`, `cursor`); it comes from the request's
//! companion `<request>.paging.json`, the collection default
//! (`extensions.requesttailor.paging`) or a plugin (`contributes.paging`), in that order of
//! precedence ([`rules::effective`]). The core knows no vendor: a plugin ships what
//! `@odata.nextLink` or `d.__next` mean as data.
//!
//! Limits are mandatory ([`PagingLimits`], hard maxima that no setting lifts). Only reads
//! follow, only to the origin of the request; every page goes through `send_with` (prepare,
//! guard, plugin hooks), so the send guard answers for each page and a "no" ends the run with
//! what was loaded. Between pages the run waits for what the last response said about rate
//! limits (`SendReport.rate_limit`), within the time limit.
//!
//! The result is one report: the merged body (only the arrays at the merge path grow; every
//! other field is the first page's), kept on the Rust side like any response. The history gets
//! one entry per run, never per page. `notes` say how many pages and why the run stopped.
//!
//! For a plugin: `Contributions.paging` is data (hosts, content types, a URL glob and a
//! [`PagingSpec`]); a rule of the user always wins, and the plugin's is offered with its name.

use serde::{Deserialize, Serialize};

pub use rt_core::paging::{CursorParam, CursorPlace, PagingRule, PagingSpec};

pub mod follow;
mod merge;
mod next;
pub mod rules;

use rt_core::send::{SendOptions, SendReport};
use rt_core::services::Services;
use rt_core::transport::Transport;

use crate::runner::RunSnapshot;
use crate::{AppError, ResponseStore, SendInput, StoredResponse};

pub use follow::{Follow, follow};
pub use rules::MatchContext;

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

/// A rule a plugin contributes, with the plugin it comes from. The shells read these from the
/// manifests of the active plugins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPaging {
    pub plugin: String,
    pub contribution: rt_plugin::api::PagingContribution,
}

/// The page `Next` continues from: its report and its whole body.
#[derive(Debug, Clone)]
pub struct PreviousPage {
    pub report: SendReport,
    pub body: Vec<u8>,
}

/// The page kept as response `id`.
pub fn previous_page(store: &ResponseStore, id: u64) -> Result<PreviousPage, AppError> {
    let report = store.report(id)?.clone();
    let size = report.response.as_ref().map_or(0, |r| r.size);
    let len = u32::try_from(size)
        .map_err(|_| AppError::Shell("The page is too large to continue from.".into()))?;
    let body = store.bytes(id, 0, len)?;
    Ok(PreviousPage { report, body })
}

/// What a run made, before its body goes into the response store.
#[derive(Debug, Clone)]
pub struct PagedRun {
    pub report: SendReport,
    pub pages: u32,
    pub stopped: PagingStop,
    pub more: bool,
}

impl PagedRun {
    /// Keeps the body in `store` and gives the result for the UI.
    pub fn into_result(self, store: &mut ResponseStore) -> PagedResult {
        PagedResult {
            response: store.insert(self.report),
            pages: self.pages,
            stopped: self.stopped,
            more: self.more,
        }
    }
}

fn slot_mut<'a>(
    items: &'a mut [rt_core::model::Item],
    path: &str,
) -> Option<&'a mut rt_core::model::RequestSpec> {
    for item in items {
        match item {
            rt_core::model::Item::Request(r) if r.path == path => return Some(&mut r.spec),
            rt_core::model::Item::Folder(f) => {
                if let Some(found) = slot_mut(&mut f.items, path) {
                    return Some(found);
                }
            }
            rt_core::model::Item::Request(_) => {}
        }
    }
    None
}

/// Puts `spec` in place of the request at `path` of `collection` (the page to send).
pub fn set_spec(
    collection: &mut rt_core::model::Collection,
    path: &str,
    spec: rt_core::model::RequestSpec,
) -> Result<(), AppError> {
    let slot = slot_mut(&mut collection.items, path)
        .ok_or_else(|| AppError::Shell(format!("There is no request {path}.")))?;
    *slot = spec;
    Ok(())
}

/// Follows the pages of `run.path` on a snapshot of the collection: the app's and the
/// browser's way. Each page is one run of the request with the services given (so the guard,
/// the cookies, the tokens and the plugins are those of a normal send).
#[allow(clippy::too_many_arguments)]
pub async fn run_on_snapshot<T: Transport>(
    snapshot: &RunSnapshot,
    run: &PagingRun,
    rule: &PagingSpec,
    previous: Option<PreviousPage>,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<PagedRun, AppError> {
    let spec = snapshot
        .collection
        .request(&run.path)
        .map(|r| r.request.spec.clone())
        .ok_or_else(|| AppError::Shell(format!("There is no request {}.", run.path)))?;
    let mut work = snapshot.clone();
    let paths = [run.path.clone()];
    follow(
        Follow {
            spec: &spec,
            rule,
            mode: run.mode,
            previous,
            limits: run.limits.clone().unwrap_or_default(),
            first: None,
        },
        async |page: rt_core::model::RequestSpec| {
            let slot = slot_mut(&mut work.collection.items, &run.path)
                .ok_or_else(|| "the request vanished".to_owned())?;
            *slot = page;
            let outcome = crate::compare::run_snapshot(
                &work,
                None,
                Some(&paths),
                transport,
                options,
                services,
                &mut |_| {},
                cancelled,
            )
            .await;
            outcome
                .steps
                .into_iter()
                .next()
                .map(|s| s.report)
                .ok_or_else(|| "the page was not sent".to_owned())
        },
        cancelled,
        &mut |_| {},
    )
    .await
}
