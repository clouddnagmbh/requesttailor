// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Consumer contracts without a broker (wave 19, strand F): `Backend.consumers`,
//! `rt verify-consumers`. A *consumer collection* is itself the contract: its requests,
//! assertions, schema checks and snapshots say what the consumer expects. The **provider's**
//! repository lists its consumers in `extensions.requesttailor.consumers` of
//! `opencollection.yml` (`rt-format/src/consumers_config.rs`):
//!
//! ```yaml
//! extensions:
//!   requesttailor:
//!     consumers:
//!       - name: billing
//!         source: ../billing-collection     # a local path or a Git URL
//!         ref: main                         # optional branch, tag or commit
//!         path: api-tests                   # optional folder of the collection in the repo
//!         environment: ci                   # an environment of the consumer collection
//!         vars: { baseUrl: "{{provider.baseUrl}}" }
//! ```
//!
//! `verify` fetches each consumer (a local path, or, **natively only**, a shallow clone with
//! `rt-git` into `consumers/<hash>/` of the data folder; the resolved commit goes into the
//! report; a token comes from `RT_GIT_TOKEN`), takes its environment with `baseUrl` set to
//! the built provider the pipeline started, runs it through the runner (scripts only with
//! trust or `--trust-scripts`), and reports per consumer and per request which assertion
//! broke and how it differs, not the response itself (masked, shortened).
//!
//! Limits: [`MAX_CONSUMERS`] consumers, [`MAX_REQUESTS`] requests per run, 10 minutes
//! (hard 60), clones shallow and at most 256 MiB each. Writing requests of consumers only
//! with `allow_writes` and never against production; the guard is asked per request.
//!
//! Layout: [`run`] runs one loaded consumer through the runner (any target, the same code as
//! `rt run`), [`report`] writes the text and GitHub forms, `fetch` (native only) finds a
//! consumer on disk or clones it. The shells decide where consumers come from; the web shell
//! only has the collections of its set.

mod report;
mod run;

#[cfg(not(target_arch = "wasm32"))]
pub mod fetch;

pub use report::{github, text};
pub use run::{Budget, DEFAULT_MINUTES, HARD_MINUTES, Loaded, run_consumer};

use rt_format::FileSet;

use crate::AppError;

use serde::{Deserialize, Serialize};

pub const MAX_CONSUMERS: usize = 50;
pub const MAX_REQUESTS: usize = 5_000;

/// One consumer as the provider's `opencollection.yml` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerRef {
    pub name: String,
    /// A local path (relative to the provider's collection) or a Git URL.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "ref")]
    pub git_ref: Option<String>,
    /// The folder of the consumer collection inside the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The environment of the consumer collection to run.
    pub environment: String,
    /// Variables set on top of the environment (`{{provider.baseUrl}}` is the base URL).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vars: Vec<(String, String)>,
}

/// What to verify (`ConsumersApi.verify`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerVerifyRequest {
    /// The built provider's base URL.
    pub base_url: String,
    /// Only these consumers (names); empty: all.
    #[serde(default)]
    pub only: Vec<String>,
    /// Fetch remote consumers again.
    #[serde(default)]
    pub fetch: bool,
    /// Keep going after a broken consumer (the default).
    #[serde(default = "yes")]
    pub keep_going: bool,
    /// Let consumers' scripts run (`--trust-scripts`).
    #[serde(default)]
    pub trust_scripts: bool,
    /// Let consumers' writing requests run (never against production).
    #[serde(default)]
    pub allow_writes: bool,
    /// Time limit of the whole run in minutes (default [`DEFAULT_MINUTES`], at most
    /// [`HARD_MINUTES`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_minutes: Option<u32>,
}

fn yes() -> bool {
    true
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The consumers the provider's `opencollection.yml` in `files` lists.
pub fn list(files: &FileSet) -> Result<Vec<ConsumerRef>, AppError> {
    Ok(rt_format::consumers_config::configured(files)?
        .into_iter()
        .map(|e| ConsumerRef {
            name: e.name,
            source: e.source,
            git_ref: e.git_ref,
            path: e.path,
            environment: e.environment,
            vars: e.vars,
        })
        .collect())
}

/// The consumers `only` names (empty: all), in the file's order. A name that is not listed is
/// an error, so a typo never means "verified nothing".
pub fn select<'a>(
    all: &'a [ConsumerRef],
    only: &[String],
) -> Result<Vec<&'a ConsumerRef>, AppError> {
    if let Some(unknown) = only.iter().find(|n| !all.iter().any(|c| &c.name == *n)) {
        return Err(AppError::Format(format!(
            "no consumer named {unknown:?} (listed: {})",
            all.iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(all
        .iter()
        .filter(|c| only.is_empty() || only.contains(&c.name))
        .collect())
}

/// One request of a consumer that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerFailure {
    pub request_path: String,
    /// `GET /orders/{id}` as written in the consumer collection.
    pub request: String,
    /// The assertion or check that broke.
    pub assertion: String,
    /// "expected `$.items[*].price` number, got string"; masked and shortened.
    pub difference: String,
}

/// The result for one consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerResult {
    pub name: String,
    /// The commit that was run (for a clone), else `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Requests that were sent.
    pub requests: u32,
    /// Writing requests (and others) the run left out: writes need `allow_writes` and never
    /// run against production.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skipped: u32,
    pub failures: Vec<ConsumerFailure>,
    /// The consumer could not be fetched or run: why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The report of `verify` (the JSON of `rt verify-consumers --out json`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ConsumerReport {
    pub consumers: Vec<ConsumerResult>,
}

impl ConsumerReport {
    /// At least one consumer broke (or could not run): `rt verify-consumers` exits with
    /// `EXIT_CONSUMERS`.
    pub fn broken(&self) -> bool {
        self.consumers
            .iter()
            .any(|c| !c.failures.is_empty() || c.error.is_some())
    }
}
