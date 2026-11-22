// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Runs of a collection at a Git revision, and the comparison of two runs (wave 12).
//!
//! A run at a revision reads the collection from Git's object store (`GitApi.treeAt`), never
//! checks anything out, and sends with today's environment, secrets and services, like the
//! runner. The PR review (strand E) uses the same run for the changed requests of a PR.
//!
//! Owner: wave 12 interface; implementation strand C (also `bisect`).

use std::collections::BTreeMap;

use rt_core::model::{Collection, Environment};
use rt_core::runner::{RunEvent, RunPlan, RunStep, RunSummary};
use rt_core::send::{ReportBody, SendOptions, SendReport};
use rt_core::services::Services;
use rt_core::transport::Transport;
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::git::{GitCommit, GitTree};
use crate::runner::RunSnapshot;

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

// ---------------------------------------------------------------------------------------
// Running at a revision

/// The collection of a revision with today's environments: the requests, folders and
/// collection settings come from the commit, the environments (and with them the secret
/// values and client certificates they carry) from `environments`, the user's current ones.
/// A revision only knows references to secrets, never values.
pub fn collection_at(tree: &GitTree, environments: &[Environment]) -> Result<Collection, AppError> {
    let doc = rt_format::read_collection(&tree.to_file_set())
        .map_err(|e| AppError::Format(format!("the collection at {}: {e}", short(&tree.oid))))?;
    let mut collection = doc.model().clone();
    collection.environments = environments.to_vec();
    Ok(collection)
}

/// The run snapshot of a revision: its requests, today's environments, variables and files.
pub fn snapshot_at(today: &RunSnapshot, tree: &GitTree) -> Result<RunSnapshot, AppError> {
    Ok(RunSnapshot {
        collection: collection_at(tree, &today.collection.environments)?,
        environment: today.environment.clone(),
        vars: today.vars.clone(),
        files: today.files.clone(),
    })
}

fn short(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

/// A run at a revision with the full reports (no body cut), as the comparison needs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionOutcome {
    pub steps: Vec<RunStep>,
    pub summary: RunSummary,
}

/// The paths a run at `snapshot` covers: `paths` that exist there, else the folder's requests.
pub fn run_paths(
    snapshot: &RunSnapshot,
    folder: Option<&str>,
    paths: Option<&[String]>,
) -> Vec<String> {
    match paths {
        Some(paths) => {
            let all = snapshot.plan(None);
            paths.iter().filter(|p| all.contains(p)).cloned().collect()
        }
        None => snapshot.plan(folder),
    }
}

/// Runs `paths` (or the folder) of `snapshot` once, without data, collecting the steps.
#[allow(clippy::too_many_arguments)]
pub async fn run_snapshot<T: Transport>(
    snapshot: &RunSnapshot,
    folder: Option<&str>,
    paths: Option<&[String]>,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    on_event: &mut dyn FnMut(&RunEvent),
    cancelled: &dyn Fn() -> bool,
) -> RevisionOutcome {
    let plan = RunPlan {
        requests: run_paths(snapshot, folder, paths),
        iterations: 1,
        ..Default::default()
    };
    let mut steps = Vec::new();
    let mut collect = |event: RunEvent| {
        on_event(&event);
        if let RunEvent::Step { step } = event {
            steps.push(*step);
        }
    };
    let summary = snapshot
        .run(&plan, transport, options, services, &mut collect, cancelled)
        .await;
    RevisionOutcome { steps, summary }
}

/// The run as the UI gets it (body previews only).
pub fn revision_run(commit: GitCommit, outcome: RevisionOutcome) -> RevisionRun {
    let steps = outcome
        .steps
        .into_iter()
        .map(|step| {
            match crate::runner::for_view(RunEvent::Step {
                step: Box::new(step),
            }) {
                RunEvent::Step { step } => *step,
                _ => unreachable!("for_view keeps the kind of event"),
            }
        })
        .collect();
    RevisionRun {
        commit,
        steps,
        summary: outcome.summary,
    }
}

/// Runs the requests of `request` at one revision (`CompareApi.run`).
#[allow(clippy::too_many_arguments)]
pub async fn run_revision<T: Transport>(
    today: &RunSnapshot,
    commit: GitCommit,
    tree: &GitTree,
    request: &RevisionRunRequest,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    on_event: &mut dyn FnMut(&RunEvent),
    cancelled: &dyn Fn() -> bool,
) -> Result<RevisionRun, AppError> {
    let snapshot = snapshot_at(today, tree)?;
    let outcome = run_snapshot(
        &snapshot,
        request.folder.as_deref(),
        request.paths.as_deref(),
        transport,
        options,
        services,
        on_event,
        cancelled,
    )
    .await;
    Ok(revision_run(commit, outcome))
}

/// Runs the same requests at two revisions and compares the answers (`CompareApi.compare`).
/// Without `paths`, the requests are those of the folder at either revision.
#[allow(clippy::too_many_arguments)]
pub async fn compare_revisions<T: Transport>(
    today: &RunSnapshot,
    from: (GitCommit, &GitTree),
    to: (GitCommit, &GitTree),
    request: &CompareRequest,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<RunComparison, AppError> {
    let old = snapshot_at(today, from.1)?;
    let new = snapshot_at(today, to.1)?;
    let folder = request.folder.as_deref();
    let paths: Vec<String> = match &request.paths {
        Some(p) => p.clone(),
        None => {
            let mut all = old.plan(folder);
            for p in new.plan(folder) {
                if !all.contains(&p) {
                    all.push(p);
                }
            }
            all
        }
    };
    let run = async |snapshot: &RunSnapshot| {
        run_snapshot(
            snapshot,
            None,
            Some(&paths),
            transport,
            options,
            services,
            &mut |_| {},
            cancelled,
        )
        .await
    };
    let before = run(&old).await;
    let after = if cancelled() {
        RevisionOutcome {
            steps: Vec::new(),
            summary: RunSummary {
                stopped: true,
                ..Default::default()
            },
        }
    } else {
        run(&new).await
    };
    Ok(RunComparison {
        from: from.0,
        to: to.0,
        requests: compare_steps(
            &paths,
            |p| name_of(&old, p),
            |p| name_of(&new, p),
            &before.steps,
            &after.steps,
        ),
        from_summary: before.summary,
        to_summary: after.summary,
    })
}

fn name_of(snapshot: &RunSnapshot, path: &str) -> Option<String> {
    snapshot
        .collection
        .request(path)
        .map(|c| c.request.spec.name.clone())
}

// ---------------------------------------------------------------------------------------
// Comparing two runs

/// Headers that differ on every response and carry no information about the API.
const VOLATILE_HEADERS: &[&str] = &[
    "date",
    "age",
    "server-timing",
    "x-request-id",
    "x-correlation-id",
    "x-trace-id",
    "traceparent",
    "content-length",
    "keep-alive",
    "connection",
];

/// Most body changes reported per request.
const BODY_CHANGE_LIMIT: usize = 200;

/// Compares the steps of two runs request by request, in the order of `paths`. A request
/// missing on one side (not in that revision, or not run) has `None` there.
pub fn compare_steps(
    paths: &[String],
    old_name: impl Fn(&str) -> Option<String>,
    new_name: impl Fn(&str) -> Option<String>,
    from: &[RunStep],
    to: &[RunStep],
) -> Vec<RequestComparison> {
    let first = |steps: &'_ [RunStep]| -> BTreeMap<String, usize> {
        let mut map = BTreeMap::new();
        for (i, s) in steps.iter().enumerate() {
            map.entry(s.request_path.clone()).or_insert(i);
        }
        map
    };
    let (a, b) = (first(from), first(to));
    paths
        .iter()
        .map(|path| {
            let old = a.get(path).map(|&i| &from[i]);
            let new = b.get(path).map(|&i| &to[i]);
            let name = new
                .map(|s| s.request_name.clone())
                .or_else(|| old.map(|s| s.request_name.clone()))
                .or_else(|| new_name(path))
                .or_else(|| old_name(path))
                .unwrap_or_else(|| path.clone());
            compare_one(path, name, old.map(|s| &s.report), new.map(|s| &s.report))
        })
        .collect()
}

fn compare_one(
    path: &str,
    name: String,
    old: Option<&SendReport>,
    new: Option<&SendReport>,
) -> RequestComparison {
    let status = {
        let o = old.and_then(|r| r.response.as_ref()).map(|r| r.status);
        let n = new.and_then(|r| r.response.as_ref()).map(|r| r.status);
        (o != n).then_some(ValueChange { old: o, new: n })
    };
    let headers = match (
        old.and_then(|r| r.response.as_ref()),
        new.and_then(|r| r.response.as_ref()),
    ) {
        (Some(o), Some(n)) => header_changes(&o.headers, &n.headers),
        _ => Vec::new(),
    };
    let (body, body_text) = match (
        old.and_then(|r| r.response.as_ref()),
        new.and_then(|r| r.response.as_ref()),
    ) {
        (Some(o), Some(n)) => body_changes(&o.body, &n.body),
        _ => (Vec::new(), false),
    };
    let tests = test_changes(old, new);
    let error = {
        let o = old.and_then(|r| r.error.as_ref()).map(error_text);
        let n = new.and_then(|r| r.error.as_ref()).map(error_text);
        (o != n).then_some(ValueChange { old: o, new: n })
    };
    // A request missing on one side (no report) is different even when it has no response.
    let missing = old.is_some() != new.is_some();
    let same = !missing
        && status.is_none()
        && headers.is_empty()
        && body.is_empty()
        && !body_text
        && tests.is_empty()
        && error.is_none();
    RequestComparison {
        path: path.to_owned(),
        name,
        same,
        status,
        headers,
        body,
        body_text,
        tests,
        error,
    }
}

fn error_text(e: &rt_core::send::SendError) -> String {
    use rt_core::send::SendError::*;
    match e {
        NotFound(m) | Prepare(m) | Auth(m) | Plugin(m) | Script(m) => m.clone(),
        Blocked { reason, .. } => reason.clone(),
        Transport(t) => t.to_string(),
    }
}

fn header_changes(old: &[(String, String)], new: &[(String, String)]) -> Vec<HeaderChange> {
    fn fold(headers: &[(String, String)]) -> BTreeMap<String, (String, String)> {
        let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
        for (name, value) in headers {
            let key = name.to_ascii_lowercase();
            if VOLATILE_HEADERS.contains(&key.as_str()) {
                continue;
            }
            map.entry(key)
                .and_modify(|(_, v)| {
                    v.push_str(", ");
                    v.push_str(value);
                })
                .or_insert_with(|| (name.clone(), value.clone()));
        }
        map
    }
    let (a, b) = (fold(old), fold(new));
    let mut out = Vec::new();
    for key in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
        let (o, n) = (a.get(key), b.get(key));
        if o.map(|x| &x.1) != n.map(|x| &x.1) {
            out.push(HeaderChange {
                name: n.or(o).map(|x| x.0.clone()).unwrap_or_default(),
                old: o.map(|x| x.1.clone()),
                new: n.map(|x| x.1.clone()),
            });
        }
    }
    out.sort_by_key(|h| h.name.to_ascii_lowercase());
    out
}

/// `(changes, differs_as_text)`.
fn body_changes(old: &ReportBody, new: &ReportBody) -> (Vec<BodyChange>, bool) {
    if old == new {
        return (Vec::new(), false);
    }
    if let (ReportBody::Utf8(o), ReportBody::Utf8(n)) = (old, new)
        && let (Ok(o), Ok(n)) = (
            serde_json::from_str::<serde_json::Value>(o),
            serde_json::from_str::<serde_json::Value>(n),
        )
    {
        let mut out = Vec::new();
        json_diff(&o, &n, String::new(), &mut out);
        return (out, false);
    }
    (Vec::new(), true)
}

fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn json_diff(
    old: &serde_json::Value,
    new: &serde_json::Value,
    pointer: String,
    out: &mut Vec<BodyChange>,
) {
    use serde_json::Value;
    if out.len() >= BODY_CHANGE_LIMIT || old == new {
        return;
    }
    match (old, new) {
        (Value::Object(o), Value::Object(n)) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.extend(n.keys().filter(|k| !o.contains_key(*k)));
            keys.sort();
            for key in keys {
                let at = format!("{pointer}/{}", pointer_token(key));
                match (o.get(key), n.get(key)) {
                    (Some(a), Some(b)) => json_diff(a, b, at, out),
                    (a, b) => out.push(BodyChange {
                        pointer: at,
                        old: a.cloned(),
                        new: b.cloned(),
                    }),
                }
            }
        }
        (Value::Array(o), Value::Array(n)) => {
            for i in 0..o.len().max(n.len()) {
                let at = format!("{pointer}/{i}");
                match (o.get(i), n.get(i)) {
                    (Some(a), Some(b)) => json_diff(a, b, at, out),
                    (a, b) => out.push(BodyChange {
                        pointer: at,
                        old: a.cloned(),
                        new: b.cloned(),
                    }),
                }
            }
        }
        _ => out.push(BodyChange {
            pointer,
            old: Some(old.clone()),
            new: Some(new.clone()),
        }),
    }
}

/// Tests (by name) and assertions (by their text) whose outcome differs.
fn test_changes(old: Option<&SendReport>, new: Option<&SendReport>) -> Vec<TestChange> {
    fn outcomes(report: Option<&SendReport>) -> BTreeMap<String, bool> {
        let mut map = BTreeMap::new();
        if let Some(r) = report {
            for t in &r.tests {
                map.insert(t.name.clone(), t.passed);
            }
            for a in &r.assertions {
                let name = if a.expected.is_empty() {
                    format!("{} {}", a.expression, a.operator)
                } else {
                    format!("{} {} {}", a.expression, a.operator, a.expected)
                };
                map.insert(name, a.passed);
            }
        }
        map
    }
    let (a, b) = (outcomes(old), outcomes(new));
    let mut out = Vec::new();
    for name in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
        let (o, n) = (a.get(name).copied(), b.get(name).copied());
        if o != n {
            out.push(TestChange {
                name: name.clone(),
                old: o,
                new: n,
            });
        }
    }
    out
}
