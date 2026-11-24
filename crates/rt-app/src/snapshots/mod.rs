// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests (wave 17, strand B): freeze a response, compare later responses with it,
//! ignore volatile paths by hand. This module holds the types and the file format, the
//! interface the strands share; the comparison and the shells' commands come with strand B.
//!
//! # Files are the truth
//!
//! A snapshot is a companion file next to the request, `<request name>.snap.json`, plain JSON
//! with fixed keys in a stable order (a diff needs no tool). The request file changes by
//! exactly one assertion in `runtime.assertions`: `expression: res`, `operator:
//! matchesSnapshot` ([`MATCHES_SNAPSHOT`]), `value`: the companion file relative to the
//! request file. Older builds report the unknown operator as a **failure**, never as a silent
//! pass. The reader of the collection only reads `*.yml`; companion files go through
//! `Backend.companions` (staged, written with `save`).
//!
//! ```json
//! { "version": 1, "status": 200, "headers": { "content-type": "application/json" },
//!   "body": { "…": "…" }, "ignore": ["$.items[*].createdAt"] }
//! ```
//!
//! # Rules the strand implements
//!
//! - Headers: only [`HEADER_ALLOWLIST`] (`etag` only on request); never `date`, `set-cookie`
//!   or anything else.
//! - Body: JSON as is; other text as a JSON string up to
//!   [`crate::compare::SNAPSHOT_BODY_LIMIT`]; beyond that [`SnapshotBody::Digest`].
//! - `ignore` takes paths of the expression language of `rt_core::assert`
//!   (`res.body.items[0].id`, `$`-style as in the example) with `[*]` for every array
//!   element; a path without a wildcard must match what `assert` resolves exactly.
//! - Secrets: every secret value of the environment becomes `***` before freezing; the
//!   comparison masks the fresh response the same way. A snapshot that
//!   `crate::secret_scan` flags is not written (`AppError::SecretsFound`).
//! - Freezing is a command of the app and `rt snapshot update`, never part of a send or `rt
//!   run`; a missing file fails the assertion with `no snapshot yet; run `rt snapshot
//!   update``.

use serde::{Deserialize, Serialize};

use rt_core::model::{Assertion, RequestSpec};
use rt_core::send::ReportBody;

use crate::workspace::Workspace;
use crate::{AppError, ResponseStore, secret_scan};

pub use rt_core::assert::MATCHES_SNAPSHOT;
pub use rt_core::snapshot::{
    HEADER_ALLOWLIST, SNAPSHOT_VERSION, Snapshot, SnapshotBody, SnapshotDiff, SnapshotDiffKind,
    check_ignore, companion_path, default_value,
};

/// What comparing a response with the snapshot of a request found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotState {
    /// Equal after the ignored paths.
    Matches,
    Differs,
    /// The assertion names a file that does not exist.
    Missing,
    /// The request has no snapshot assertion and no `.snap.json` beside it.
    None,
}

/// The result of `SnapshotApi.check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCheck {
    pub request_path: String,
    /// The companion file, relative to the collection; empty for [`SnapshotState::None`].
    pub snapshot: String,
    pub state: SnapshotState,
    /// Empty unless [`SnapshotState::Differs`]; ignored paths are not listed.
    pub differences: Vec<SnapshotDiff>,
}

/// The result of freezing (or accepting a new response): what is staged, unsaved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFreeze {
    pub request_path: String,
    /// The companion file, relative to the collection.
    pub snapshot: String,
    /// The request got its assertion with this freeze (`false`: it already had one).
    pub assertion_added: bool,
    pub content: Snapshot,
}

/// What the user chose when freezing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FreezeOptions {
    /// Keep the `etag` header (off by default).
    #[serde(default)]
    pub keep_etag: bool,
    /// The `value` of the new assertion; `None`: [`default_value`]. Ignored when the request
    /// already has a snapshot assertion.
    #[serde(default)]
    pub file: Option<String>,
}

/// A request with a snapshot, for `list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub request_path: String,
    /// The companion file, relative to the collection.
    pub snapshot: String,
    /// The file exists (staged or saved).
    pub exists: bool,
}

/// The `value` of the request's snapshot assertion; `None` if it has none.
fn assertion_value(spec: &RequestSpec) -> Option<String> {
    spec.assertions
        .iter()
        .find(|a| a.operator.trim() == MATCHES_SNAPSHOT)
        .map(|a| match a.value.trim() {
            "" => String::new(),
            v => v.to_owned(),
        })
}

/// The companion file (relative to the collection) of the request: the assertion's file, or
/// the default file if it exists beside a request without the assertion.
fn locate(workspace: &Workspace, path: &str, spec: &RequestSpec) -> Option<String> {
    match assertion_value(spec) {
        Some(value) => {
            let value = if value.is_empty() {
                default_value(path)
            } else {
                value
            };
            Some(companion_path(path, &value))
        }
        None => {
            let file = companion_path(path, &default_value(path));
            workspace.companion_file(&file).map(|_| file)
        }
    }
}

/// The snapshot of the request at `path`; `None` if it has none (or the file is missing).
pub fn read(workspace: &Workspace, path: &str) -> Result<Option<Snapshot>, AppError> {
    let spec = workspace.request(path)?;
    let Some(file) = locate(workspace, path, &spec) else {
        return Ok(None);
    };
    let Some(bytes) = workspace.companion_file(&file) else {
        return Ok(None);
    };
    rt_core::snapshot::parse(&String::from_utf8_lossy(bytes))
        .map(Some)
        .map_err(|e| AppError::Shell(format!("the snapshot {file} cannot be read: {e}")))
}

/// Status, headers and body of a stored response.
type StoredParts = (u16, Vec<(String, String)>, Vec<u8>);

/// The stored response as bytes for the engine.
fn view_of(responses: &ResponseStore, id: u64) -> Result<StoredParts, AppError> {
    view_of_report(responses.report(id)?)
}

/// The status, headers and body of a report's response, for the engine (`rt snapshot
/// update`, the app). Refuses a report without a response or with a cut body.
pub fn view_of_report(report: &rt_core::send::SendReport) -> Result<StoredParts, AppError> {
    let response = report
        .response
        .as_ref()
        .ok_or_else(|| AppError::Shell("the request got no response".into()))?;
    if response.truncated {
        return Err(AppError::Shell(
            "the response was cut at the body limit; a snapshot needs all of it".into(),
        ));
    }
    let body = match &response.body {
        ReportBody::Utf8(text) => text.clone().into_bytes(),
        ReportBody::Base64(text) => {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(text)
                .map_err(|e| AppError::Shell(e.to_string()))?
        }
    };
    Ok((response.status, response.headers.clone(), body))
}

/// The text of the file for `snapshot`, or `SecretsFound` when it contains a credential
/// (never written).
pub fn file_text(file: &str, snapshot: &Snapshot) -> Result<String, AppError> {
    let text = rt_core::snapshot::to_text(snapshot);
    let findings = secret_scan::scan(&[(file, &text)]);
    if !findings.is_empty() {
        return Err(AppError::SecretsFound(secret_scan::refusal(&findings)));
    }
    Ok(text)
}

fn stage(workspace: &mut Workspace, file: &str, snapshot: &Snapshot) -> Result<(), AppError> {
    let text = file_text(file, snapshot)?;
    workspace.stage_companion(file, text.into_bytes())
}

/// A freeze worked out but not applied: the shells compute it with the kept responses (shared
/// borrow) and apply it to the workspace afterwards.
#[derive(Debug, Clone)]
pub struct FreezePlan {
    freeze: SnapshotFreeze,
    text: String,
    /// The request with the new assertion; `None`: it has one.
    spec: Option<RequestSpec>,
}

impl FreezePlan {
    /// What applying the plan stages.
    pub fn freeze(&self) -> &SnapshotFreeze {
        &self.freeze
    }
}

/// Freezes the stored response `response_id` as the snapshot of the request at `path`, staged
/// and unsaved: the file, and the assertion if the request has none. An existing snapshot
/// keeps its ignored paths (accepting a new response). `secrets` are the secret values of the
/// environment the response was sent with ([`environment_of`]); the stored body still has them,
/// and the file gets `***` instead.
pub fn freeze(
    workspace: &mut Workspace,
    responses: &ResponseStore,
    path: &str,
    response_id: u64,
    options: &FreezeOptions,
    secrets: &[String],
) -> Result<SnapshotFreeze, AppError> {
    let plan = plan_freeze(workspace, responses, path, response_id, options, secrets)?;
    apply_freeze(workspace, plan)
}

/// The first half of [`freeze`]: reads, writes nothing.
pub fn plan_freeze(
    workspace: &Workspace,
    responses: &ResponseStore,
    path: &str,
    response_id: u64,
    options: &FreezeOptions,
    secrets: &[String],
) -> Result<FreezePlan, AppError> {
    let mut spec = workspace.request(path)?;
    let existing = read(workspace, path)?;
    let (status, headers, body) = view_of(responses, response_id)?;
    let view = rt_core::snapshot::ResponseView {
        status,
        headers: &headers,
        body: &body,
    };
    let keep_etag = options.keep_etag
        || existing
            .as_ref()
            .is_some_and(|s| s.headers.contains_key("etag"));
    let content = rt_core::snapshot::freeze(
        &view,
        secrets,
        &rt_core::snapshot::Freeze {
            keep_etag,
            ignore: existing.map(|s| s.ignore).unwrap_or_default(),
        },
    );
    let added = assertion_value(&spec).is_none();
    let file = match assertion_value(&spec) {
        Some(value) if !value.is_empty() => companion_path(path, &value),
        Some(_) => companion_path(path, &default_value(path)),
        None => {
            let value = match options.file.as_deref().map(str::trim) {
                Some(v) if !v.is_empty() => v.to_owned(),
                _ => default_value(path),
            };
            check_value(&value)?;
            spec.assertions.push(Assertion {
                expression: "res".into(),
                operator: MATCHES_SNAPSHOT.into(),
                value: value.clone(),
                description: None,
                enabled: true,
            });
            companion_path(path, &value)
        }
    };
    let text = file_text(&file, &content)?;
    Ok(FreezePlan {
        freeze: SnapshotFreeze {
            request_path: path.to_owned(),
            snapshot: file,
            assertion_added: added,
            content,
        },
        text,
        spec: added.then_some(spec),
    })
}

/// The second half of [`freeze`]: stages the file and the assertion.
pub fn apply_freeze(
    workspace: &mut Workspace,
    plan: FreezePlan,
) -> Result<SnapshotFreeze, AppError> {
    workspace.stage_companion(&plan.freeze.snapshot, plan.text.into_bytes())?;
    if let Some(spec) = plan.spec {
        workspace.update_request(&plan.freeze.request_path, spec)?;
    }
    Ok(plan.freeze)
}

/// The assertion's file must stay beside the request: a name ending in `.snap.json`.
fn check_value(value: &str) -> Result<(), AppError> {
    if value.ends_with(".snap.json") && !value.starts_with('/') && !value.contains("..") {
        Ok(())
    } else {
        Err(AppError::Shell(format!(
            "{value:?} is not a snapshot file name (it must end in .snap.json, beside the request)"
        )))
    }
}

/// Replaces the ignored paths of the snapshot of the request at `path` (staged, unsaved).
pub fn set_ignore(
    workspace: &mut Workspace,
    path: &str,
    ignore: Vec<String>,
) -> Result<Snapshot, AppError> {
    for p in &ignore {
        check_ignore(p).map_err(|e| AppError::Shell(format!("ignore path {p:?}: {e}")))?;
    }
    let spec = workspace.request(path)?;
    let file = locate(workspace, path, &spec)
        .ok_or_else(|| AppError::Shell("the request has no snapshot yet".into()))?;
    let mut snapshot = read(workspace, path)?
        .ok_or_else(|| AppError::Shell("the request has no snapshot yet".into()))?;
    let mut seen = Vec::new();
    for p in ignore {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    snapshot.ignore = seen;
    stage(workspace, &file, &snapshot)?;
    Ok(snapshot)
}

/// The environment the stored response was sent with: the shell reads its secret values for
/// [`freeze`] and [`check`].
pub fn environment_of(
    responses: &ResponseStore,
    response_id: u64,
) -> Result<Option<String>, AppError> {
    Ok(responses.report(response_id)?.environment.clone())
}

/// Compares the stored response with the snapshot of the request at `path`.
pub fn check(
    workspace: &Workspace,
    responses: &ResponseStore,
    path: &str,
    response_id: u64,
    secrets: &[String],
) -> Result<SnapshotCheck, AppError> {
    let spec = workspace.request(path)?;
    let none = |state, snapshot: String| SnapshotCheck {
        request_path: path.to_owned(),
        snapshot,
        state,
        differences: Vec::new(),
    };
    let Some(file) = locate(workspace, path, &spec) else {
        return Ok(none(SnapshotState::None, String::new()));
    };
    let Some(snapshot) = read(workspace, path)? else {
        return Ok(none(SnapshotState::Missing, file));
    };
    let (status, headers, body) = view_of(responses, response_id)?;
    let view = rt_core::snapshot::ResponseView {
        status,
        headers: &headers,
        body: &body,
    };
    let differences = rt_core::snapshot::compare(&snapshot, &view, secrets);
    Ok(SnapshotCheck {
        request_path: path.to_owned(),
        snapshot: file,
        state: if differences.is_empty() {
            SnapshotState::Matches
        } else {
            SnapshotState::Differs
        },
        differences,
    })
}

/// Every request of the collection that has a snapshot assertion.
pub fn list(workspace: &Workspace) -> Vec<SnapshotEntry> {
    let mut out = Vec::new();
    for path in request_paths(workspace) {
        let Ok(spec) = workspace.request(&path) else {
            continue;
        };
        if assertion_value(&spec).is_none() {
            continue;
        }
        let Some(file) = locate(workspace, &path, &spec) else {
            continue;
        };
        out.push(SnapshotEntry {
            exists: workspace.companion_file(&file).is_some(),
            request_path: path,
            snapshot: file,
        });
    }
    out
}

fn request_paths(workspace: &Workspace) -> Vec<String> {
    rt_core::runner::plan(workspace.collection(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_names() {
        assert_eq!(default_value("users/get.yml"), "get.snap.json");
        assert_eq!(default_value("get.yml"), "get.snap.json");
        assert_eq!(
            companion_path("users/get.yml", "get.snap.json"),
            "users/get.snap.json"
        );
        assert_eq!(companion_path("get.yml", "get.snap.json"), "get.snap.json");
    }

    #[test]
    fn the_file_format_has_the_documented_keys() {
        let snap = Snapshot {
            version: SNAPSHOT_VERSION,
            status: 200,
            headers: std::collections::BTreeMap::from([(
                "content-type".into(),
                "application/json".into(),
            )]),
            body: SnapshotBody::Value(serde_json::json!({ "a": 1 })),
            ignore: vec!["$.a".into()],
        };
        let text = serde_json::to_string(&snap).unwrap();
        assert_eq!(
            text,
            r#"{"version":1,"status":200,"headers":{"content-type":"application/json"},"body":{"a":1},"ignore":["$.a"]}"#
        );
        assert_eq!(serde_json::from_str::<Snapshot>(&text).unwrap(), snap);
        let big = SnapshotBody::Digest {
            digest: "ab".into(),
            length: 9,
        };
        let text = serde_json::to_string(&big).unwrap();
        assert_eq!(text, r#"{"$digest":"ab","$length":9}"#);
        assert_eq!(serde_json::from_str::<SnapshotBody>(&text).unwrap(), big);
    }
}
