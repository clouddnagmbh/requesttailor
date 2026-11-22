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
//! Owner: wave 12 interface; implementation strand D. The file log, blame and the results
//! store logic here are pure (they work on texts), so the desktop shell and the web shell
//! (rt-wasm) share them.

use std::collections::BTreeMap;

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

// ---------------------------------------------------------------------------------------------
// Implementation (strand D)

/// One version of a request file as the shell read it from Git: the file log's entry and the
/// file's text at that commit (`None` where the commit deleted it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub commit: GitFileCommit,
    pub text: Option<String>,
}

/// An empty request: what the first version of a file is compared with.
const BLANK: &str =
    "info:\n  name: \"\"\n  type: http\n  seq: 1\n\nhttp:\n  method: \"\"\n  url: \"\"\n";

const COLLECTION: &str = "opencollection: 1.0.0\n\ninfo:\n  name: history\n";

/// The one request file every version is put under, so the diff sees a modification.
const REQUEST: &str = "r.yml";

fn file_set(request: &str) -> rt_format::FileSet {
    let mut set = rt_format::FileSet::default();
    set.files.insert(
        rt_format::COLLECTION_FILE.into(),
        COLLECTION.as_bytes().to_vec(),
    );
    set.files
        .insert(REQUEST.into(), request.as_bytes().to_vec());
    set
}

/// The fields `new` changes against `old` (`None`: against an empty request), as the
/// semantic diff names them (masked). Empty if a side does not parse as a request.
pub fn changes(old: Option<&str>, new: &str) -> Vec<FieldChange> {
    let diff = crate::diffs::diff(&file_set(old.unwrap_or(BLANK)), &file_set(new));
    let Ok(diff) = diff else {
        return Vec::new();
    };
    diff.changes
        .into_iter()
        .find(|c| matches!(&c.target, rt_format::diff::ChangeTarget::Request { .. }))
        .map(|c| c.fields)
        .unwrap_or_default()
}

fn entry_fields(versions: &[Version], at: usize) -> Vec<FieldChange> {
    let Some(text) = versions[at].text.as_deref() else {
        return Vec::new();
    };
    let before = versions.get(at + 1).and_then(|v| v.text.as_deref());
    changes(before, text)
}

/// The history of the request file `path` (today's path) from its `versions`, newest first
/// (`GitApi.fileLog` plus `show` per entry). `records` holds the recorded results per commit id
/// (`results`); an entry gets those of `collection` for the request path it had at its commit.
pub fn build(
    path: &str,
    collection: &str,
    versions: &[Version],
    records: &BTreeMap<String, Vec<TestRecord>>,
) -> RequestTimeline {
    let entries = versions
        .iter()
        .enumerate()
        .map(|(at, v)| TimelineEntry {
            commit: v.commit.clone(),
            fields: entry_fields(versions, at),
            tests: records
                .get(&v.commit.commit.oid)
                .map(|all| for_request(all, collection, &v.commit.path))
                .unwrap_or_default(),
        })
        .collect();
    RequestTimeline {
        path: path.to_owned(),
        entries,
    }
}

/// The records of one request in one collection, newest first.
pub fn for_request(records: &[TestRecord], collection: &str, request: &str) -> Vec<TestRecord> {
    let mut out: Vec<TestRecord> = records
        .iter()
        .filter(|r| r.collection == collection && r.request == request)
        .cloned()
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.time));
    out
}

/// For each field the request has as of the newest version, the commit that last changed it.
/// Fields come in model order. `versions` is the whole history, newest first; with a cut-off
/// history the oldest version stands for everything before it.
pub fn blame(versions: &[Version]) -> Vec<FieldBlame> {
    let Some(head) = versions.first().and_then(|v| v.text.as_deref()) else {
        return Vec::new();
    };
    let per_version: Vec<Vec<FieldChange>> = (0..versions.len())
        .map(|at| entry_fields(versions, at))
        .collect();
    let mut out = Vec::new();
    for field in changes(None, head) {
        let Some(value) = field.new else {
            continue;
        };
        let last = per_version
            .iter()
            .position(|fields| fields.iter().any(|f| f.path == field.path))
            .unwrap_or(versions.len() - 1);
        out.push(FieldBlame {
            field: field.path,
            commit: versions[last].commit.commit.clone(),
            value: Some(value),
        });
    }
    out
}

/// The request in `text` (a request file of any collection, read on its own).
pub fn spec_of(text: &str) -> Result<rt_core::model::RequestSpec, crate::AppError> {
    let doc = rt_format::read_collection(&file_set(text))?;
    doc.model()
        .request(REQUEST)
        .map(|c| c.request.spec.clone())
        .ok_or_else(|| crate::AppError::Format("the file is not a readable request".into()))
}

/// Records of a results file (one JSON line each); a line that does not parse is skipped.
pub fn parse_records(text: &str) -> Vec<TestRecord> {
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// `existing` (the text of a results file) plus `new`, with at most [`RECORDS_PER_REQUEST`]
/// records per collection and request: the oldest lines go first.
pub fn merge_records(existing: &str, new: &[TestRecord]) -> String {
    let mut all = parse_records(existing);
    all.extend(new.iter().cloned());
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
    for r in &all {
        *seen
            .entry((r.collection.clone(), r.request.clone()))
            .or_default() += 1;
    }
    let mut out = String::new();
    for r in all {
        let key = (r.collection.clone(), r.request.clone());
        let left = seen.get_mut(&key).expect("counted");
        if *left > RECORDS_PER_REQUEST {
            *left -= 1;
            continue;
        }
        out.push_str(&serde_json::to_string(&r).expect("a record serializes"));
        out.push('\n');
    }
    out
}

/// The record of one finished request of a run. Names of assertions are
/// `<expression> <operator> <expected>`; nothing else of the report is kept.
pub fn record_of_step(
    collection: &str,
    time: i64,
    source: RecordSource,
    step: &rt_core::runner::RunStep,
) -> TestRecord {
    let report = &step.report;
    let tests = report
        .tests
        .iter()
        .map(|t| RecordedTest {
            name: t.name.clone(),
            passed: t.passed,
        })
        .chain(report.assertions.iter().map(|a| RecordedTest {
            name: format!("{} {} {}", a.expression, a.operator, a.expected),
            passed: a.passed,
        }))
        .collect();
    TestRecord {
        collection: collection.to_owned(),
        request: step.request_path.clone(),
        environment: report.environment.clone(),
        time,
        source,
        passed: step.passed,
        status: report.response.as_ref().map(|r| r.status),
        tests,
    }
}

/// The records of a finished run at a revision (compare, bisect): one per request that was
/// not skipped. The run used exactly the commit's request files, so they belong under it.
pub fn records_of_steps(
    collection: &str,
    time: i64,
    source: RecordSource,
    steps: &[rt_core::runner::RunStep],
) -> Vec<TestRecord> {
    steps
        .iter()
        .filter(|s| !s.skipped)
        .map(|s| record_of_step(collection, time, source, s))
        .collect()
}

/// The records of a run, from its events: one per finished request that was not skipped.
/// Collect with [`RunRecords::observe`]; `finish` gives them once the run has ended.
#[derive(Debug, Default)]
pub struct RunRecords {
    records: Vec<TestRecord>,
}

impl RunRecords {
    pub fn observe(
        &mut self,
        collection: &str,
        time: i64,
        source: RecordSource,
        event: &rt_core::runner::RunEvent,
    ) {
        if let rt_core::runner::RunEvent::Step { step } = event
            && !step.skipped
        {
            self.records
                .push(record_of_step(collection, time, source, step));
        }
    }

    pub fn finish(self) -> Vec<TestRecord> {
        self.records
    }
}
