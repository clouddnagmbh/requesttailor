// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The test-data journal and reverting (wave 19, strand C): what a session wrote to a server,
//! so the user can put it back. `Backend.journal`.
//!
//! **Journal.** Per collection and device, **off** by default (the user switches "Track test
//! data" on). `rt_core::journal::record` hands every finished send's masked exchange to it;
//! it keeps only **successful writing** requests (`effect = Write`, status 2xx): time,
//! environment, request path, method, masked URL, status, the response `Location` (masked)
//! and ETag, an id the user or a plugin named (`id_path`, a JSONPath, or an `actions`
//! variable). **No bodies, no header values** other than `Location`. Device state in the data
//! folder: `journal/<collection-id>.jsonl`, a ring of at most [`RING_ENTRIES`] entries and
//! [`RING_DAYS`] days; never in a collection file, never in the response memory. Entries are
//! written by a background channel; the send is never blocked.
//!
//! **Manual revert (Free).** [`JournalApi::plan_manual`] fills the variables `{{journal.id}}`,
//! `{{journal.location}}`, `{{journal.etag}}` of a request the user picked and shows what
//! would be sent; [`JournalApi::execute`] sends it (guard per step, production question,
//! audit entry) and marks the entry `reverted`. No deriving, no ordering logic.
//!
//! **Compensators (the extension point Pro fills).** An extension's compensator turns an
//! entry into a [`Compensation`] (a request that undoes it) or a [`NotReversible`]; the core
//! plans in reverse order, checks the state before each step (conflict guard: a resource
//! changed by someone else is skipped, never overwritten), runs the steps under the same
//! rules as any writing request, and verifies afterwards. Plugins add rules as data
//! (`contributes.compensations`, [`JournalApi::plugin_rules`]); the core executes, the
//! provider supplies data only.
//!
//! Interface: types only.

use serde::{Deserialize, Serialize};

use rt_core::model::RequestSpec;

/// Most entries kept per collection.
pub const RING_ENTRIES: usize = 5_000;
/// Oldest entry kept, in days.
pub const RING_DAYS: u32 = 30;
/// Most steps in one plan.
pub const MAX_STEPS: usize = 500;
/// Retries of a step on a 5xx answer.
pub const MAX_RETRIES: u32 = 5;

/// Per collection and device (`JournalApi.settings`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JournalSettings {
    /// "Track test data" is on.
    #[serde(default)]
    pub enabled: bool,
    /// A JSONPath into the response body that names the created resource (`$.id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_path: Option<String>,
}

/// One successful writing request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Increasing per collection.
    pub id: u64,
    /// The session this belongs to (one per app start or CLI run).
    pub session: String,
    /// Unix milliseconds.
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    pub request_path: String,
    pub method: String,
    /// Masked.
    pub url: String,
    pub status: u16,
    /// The response `Location`, masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// The id found by `id_path`, a plugin rule or an `actions` variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    /// Set once reverted (`Unix milliseconds`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverted_at: Option<i64>,
}

/// What to list (`JournalApi.entries`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JournalQuery {
    /// At most this many, newest first (`0`: the default of 200).
    #[serde(default)]
    pub limit: u32,
    /// Entries older than this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Only entries not yet reverted.
    #[serde(default)]
    pub only_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalPage {
    pub entries: Vec<JournalEntry>,
    /// All entries of the collection that match, before `limit`.
    pub total: u64,
}

/// How a compensation puts things back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompensationKind {
    /// Delete what the entry created.
    Delete,
    /// Write the old values back (only the fields the request changed).
    Restore,
    /// Create what the entry deleted.
    Recreate,
    /// A request the user chose (manual revert).
    Manual,
}

/// A request that undoes an entry, as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Compensation {
    pub kind: CompensationKind,
    /// The request to send; its variables are already filled in.
    pub request: RequestSpec,
    /// A hash of the fields the original request touched, as the server held them right
    /// after it (the conflict guard compares the current state with it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_hash: Option<String>,
    /// A request that reads the current state for the conflict guard and the check after
    /// the step (`GET` of the resource).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<RequestSpec>,
    /// One line for the plan.
    pub description: String,
}

/// An entry that cannot be put back, with the reason ("RPC call", "no body known").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotReversible {
    pub reason: String,
}

/// What an extension answers for one entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CompensationAnswer {
    Compensation { compensation: Box<Compensation> },
    NotReversible { reason: String },
}

/// One entry with its answer (`JournalApi.plan_compensations`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompensationItem {
    pub entry_id: u64,
    pub answer: CompensationAnswer,
}

/// The plan of a manual revert (`JournalApi.plan_manual`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualRevert {
    pub entry_id: u64,
    /// A request of the collection that does the undoing, or an unsaved draft the user made.
    pub request_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepState {
    Planned,
    NotReversible,
}

/// One step of a plan: shown to the user before anything is sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevertStep {
    pub entry_id: u64,
    pub state: StepState,
    pub method: String,
    /// Masked.
    pub url: String,
    /// `rt_core::guard::effect` of the request.
    pub effect: rt_core::guard::Effect,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<Compensation>,
    /// Why not (for `NotReversible`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A plan, newest entry first (reverse order of writing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevertPlan {
    pub environment: Option<String>,
    /// The environment is production (`rt_app::agents::is_production`): executing needs the
    /// user's explicit "Revert N changes in PRODUCTION?".
    pub production: bool,
    pub steps: Vec<RevertStep>,
    /// More entries than [`MAX_STEPS`]; the rest are left for another plan.
    #[serde(default)]
    pub truncated: bool,
}

/// What the user confirmed for `execute`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RevertOptions {
    /// Production was asked and confirmed for this plan.
    #[serde(default)]
    pub production_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepResult {
    Reverted,
    /// The resource was changed by someone else; left alone.
    SkippedConflict,
    SkippedNotReversible,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepOutcome {
    pub entry_id: u64,
    pub result: StepResult,
    /// Masked.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevertOutcome {
    pub steps: Vec<StepOutcome>,
}

/// A rule a plugin ships as data (`contributes.compensations`), listed with its plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginCompensationRule {
    pub plugin: String,
    pub rule: rt_plugin::api::CompensationContribution,
}

/// The state of a resource read by a compensator's `preRead` (before the write) or after it,
/// kept **in memory only**, never saved (a restart loses it; an entry that needs it is then
/// reported not reversible). The body is masked for the secret values the app knows; a
/// compensation that would have to write masked values back is not reversible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreRead {
    pub status: u16,
    pub body: Option<String>,
    pub etag: Option<String>,
}

/// What the journal holds in memory for an entry (`JournalApi.states`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryStates {
    pub entry_id: u64,
    pub before: Option<PreRead>,
    pub after: Option<PreRead>,
}
