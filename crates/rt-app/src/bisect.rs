// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Bisect with the runner (wave 12): the first commit between a good and a bad one at which a
//! test, an assertion or a request fails. `rt bisect` and the app (`CompareApi.bisect`) share
//! the search; each tested commit is a run at a revision (`compare`), recorded like any run
//! (`timeline`).
//!
//! The candidates are the first-parent chain from `bad` back to `good`
//! (`GitApi.commits(good, bad)`), so merges are tested as a whole.
//!
//! Owner: wave 12 interface; implementation strand C.

use serde::{Deserialize, Serialize};

use crate::git::GitCommit;

/// What to bisect. The send input is passed next to it, as for `RunnerApi.run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectRequest {
    /// A revision where everything passes.
    pub good: String,
    /// A later revision where something fails.
    pub bad: String,
    /// `None` or `""`: the whole collection.
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BisectVerdict {
    Good,
    Bad,
    /// Not testable (the collection cannot be read at this commit, or the folder is missing).
    Skip,
}

/// One tested commit (progress and history of a bisect).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectStep {
    pub commit: GitCommit,
    pub verdict: BisectVerdict,
    /// What failed (request names with their failed test or assertion), for `Bad`.
    pub failed: Vec<String>,
    /// Candidates left after this step.
    pub remaining: u32,
}

/// The result of a bisect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectOutcome {
    /// The first bad commit; `None` if `bad` passes now, or only skipped commits are left.
    pub first_bad: Option<GitCommit>,
    /// Commits between good (excluded) and bad (included).
    pub candidates: u32,
    pub steps: Vec<BisectStep>,
}
