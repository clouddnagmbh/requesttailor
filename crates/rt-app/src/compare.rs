// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Runs of a collection at a Git revision, and the comparison of two runs (wave 12).
//!
//! A run at a revision reads the collection from Git's object store (`GitApi.treeAt`), never
//! checks anything out, and sends with today's environment, secrets and services, like the
//! runner. The PR review (strand E) uses the same run for the changed requests of a PR.
//!
//! Owner: wave 12 interface; implementation strand C (also `bisect`).

use rt_core::runner::{RunStep, RunSummary};
use serde::{Deserialize, Serialize};

use crate::git::GitCommit;

/// What to run at a revision (`CompareApi.run`). The send input (environment, variables) is
/// passed next to it, as for `RunnerApi.run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionRunRequest {
    pub rev: String,
    /// `None` or `""`: the whole collection (as `rt_core::runner::plan`).
    #[serde(default)]
    pub folder: Option<String>,
    /// Only these requests (paths at `rev`), in this order; overrides `folder`.
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

/// One run at a revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionRun {
    pub commit: GitCommit,
    /// Steps as the runner reports them (body previews only).
    pub steps: Vec<RunStep>,
    pub summary: RunSummary,
}

/// What to compare (`CompareApi.compare`): the same requests at two revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareRequest {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

/// An old and a new value; `None` where there is none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueChange<T> {
    pub old: Option<T>,
    pub new: Option<T>,
}

/// A header that differs (values masked as in the reports).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderChange {
    pub name: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// A JSON value of the response body that differs, by JSON Pointer (`/items/0/id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyChange {
    pub pointer: String,
    pub old: Option<serde_json::Value>,
    pub new: Option<serde_json::Value>,
}

/// A test or assertion whose outcome differs (`None`: not run on that side).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestChange {
    pub name: String,
    pub old: Option<bool>,
    pub new: Option<bool>,
}

/// One request in both runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestComparison {
    pub path: String,
    pub name: String,
    /// Nothing below differs.
    pub same: bool,
    /// `None` when the status is the same; `old`/`new` `None` where the request was not sent
    /// (missing at that revision, or no response).
    pub status: Option<ValueChange<u16>>,
    /// Headers that differ (headers that always change, like `Date`, are left out).
    pub headers: Vec<HeaderChange>,
    /// JSON bodies: the values that differ.
    pub body: Vec<BodyChange>,
    /// Bodies that are not JSON differ.
    pub body_text: bool,
    pub tests: Vec<TestChange>,
    /// Transport or preparation errors (`None` when the same).
    pub error: Option<ValueChange<String>>,
}

/// The comparison of two runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunComparison {
    pub from: GitCommit,
    pub to: GitCommit,
    pub requests: Vec<RequestComparison>,
    pub from_summary: RunSummary,
    pub to_summary: RunSummary,
}
