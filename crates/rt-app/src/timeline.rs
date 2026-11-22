// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Time travel per request (wave 12): the versions of a request file over the commits (also
//! across renames), blame per field, and test results per commit.
//!
//! # Test results per commit
//!
//! A run's results are recorded locally, never in the repository: natively in the data folder
//! (shared by the desktop app and `rt`), in the web app in IndexedDB. A run is recorded under
//! a commit only when it ran exactly that commit's files: a run of the open collection while
//! its files equal `HEAD` (no unsaved changes, no uncommitted changes in the collection), or a
//! run at a revision (compare, bisect). One JSON line per request ([`TestRecord`]) in
//! [`results_file`] of the commit; no bodies, no header values, messages masked as in the
//! reports. Commit ids are unique, so the file needs no repository key; [`TestRecord::collection`]
//! keeps two collections of one repository apart.
//!
//! Owner: wave 12 interface; implementation strand D.

use serde::{Deserialize, Serialize};

use crate::git::{GitCommit, GitFileCommit};
use rt_format::diff::{DiffValue, FieldChange, FieldPath};

/// Folder of the recorded results below the data folder.
pub const RESULTS_DIR: &str = "test-results";

/// Records kept per request and commit (oldest dropped).
pub const RECORDS_PER_REQUEST: usize = 20;

/// The results file of a commit below the data folder: `test-results/<2 hex>/<oid>.jsonl`.
pub fn results_file(oid: &str) -> String {
    let fan = oid.get(..2).unwrap_or("00");
    format!("{RESULTS_DIR}/{fan}/{oid}.jsonl")
}

/// What produced a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordSource {
    Runner,
    Compare,
    Bisect,
}

/// One test or assertion of a recorded request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedTest {
    pub name: String,
    pub passed: bool,
}

/// The result of one request in one run, recorded under a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRecord {
    /// Repository-relative path of the collection root (`""` at the repository root).
    pub collection: String,
    /// Request path relative to the collection root, as at the commit.
    pub request: String,
    pub environment: Option<String>,
    /// Unix seconds.
    pub time: i64,
    pub source: RecordSource,
    /// No error, every test and assertion passed.
    pub passed: bool,
    /// HTTP status, if a response came.
    pub status: Option<u16>,
    pub tests: Vec<RecordedTest>,
}

/// One version of a request in its history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub commit: GitFileCommit,
    /// What this commit changed in the request, field by field (against the previous version).
    pub fields: Vec<FieldChange>,
    /// Results recorded under this commit for this request, newest first.
    pub tests: Vec<TestRecord>,
}

/// The history of one request file, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestTimeline {
    /// Today's path.
    pub path: String,
    pub entries: Vec<TimelineEntry>,
}

/// Who last changed a field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldBlame {
    pub field: FieldPath,
    pub commit: GitCommit,
    /// The value as of `HEAD` (secrets as [`DiffValue::Secret`]).
    pub value: Option<DiffValue>,
}
