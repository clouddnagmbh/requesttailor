// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Comparing responses across environments (wave 18, strand A). The same requests run one
//! after the other in two or more environments (per request first environment A, then B, so
//! the time between them stays small) and the steps go to [`crate::compare::compare_steps`];
//! the comparison view of wave 14 shows the result. Nothing here is saved.
//!
//! Rules the strand implements: only requests with the effect `read`
//! ([`rt_core::guard::effect`]) are sent, writing ones are listed as skipped with a reason;
//! an environment that is production ([`crate::agents::is_production`]) needs
//! [`EnvCompareRequest::production_confirmed`] (the UI asks once); the guard is asked per
//! request; secrets, cookies, tokens and session variables stay per environment; everything
//! shown is masked ([`crate::compare::ResponseSnapshot`], `SNAPSHOT_BODY_LIMIT`).
//!
//! [`compare_environments`] (in `run.rs`) is the one implementation for the desktop app, the
//! web app (`rt-wasm`) and `rt compare-env`; the shell builds one [`EnvRun`] per environment
//! (the run snapshot with that environment's secret values, and the services of the send).

mod run;

pub use run::{EnvRun, compare_environments};

use serde::{Deserialize, Serialize};

use rt_core::runner::RunSummary;

use crate::compare::RequestComparison;

/// What to compare (`EnvCompareApi.run`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvCompareRequest {
    /// Two or more environment names; the first is the reference.
    pub environments: Vec<String>,
    /// Limits the run to a folder of the active collection (`None`: all of it).
    #[serde(default)]
    pub folder: Option<String>,
    /// Limits the run to these requests.
    #[serde(default)]
    pub paths: Option<Vec<String>>,
    /// Hide differences that are only the values of the environments' own variables (host in
    /// `Location`, base URLs in the body). Off by default.
    #[serde(default)]
    pub ignore_env_values: bool,
    /// The user confirmed sending reads to an environment marked as production.
    #[serde(default)]
    pub production_confirmed: bool,
}

/// A request that was not sent, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedRequest {
    pub path: String,
    pub name: String,
    /// e.g. `skipped (writes)`.
    pub reason: String,
}

/// The reference against one other environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvPair {
    /// The other environment.
    pub environment: String,
    pub requests: Vec<RequestComparison>,
    pub reference_summary: RunSummary,
    pub summary: RunSummary,
}

/// The result of `EnvCompareApi.run`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvComparison {
    /// The first environment of the request.
    pub reference: String,
    /// One pair per other environment, in the order asked for.
    pub pairs: Vec<EnvPair>,
    pub skipped: Vec<SkippedRequest>,
}
