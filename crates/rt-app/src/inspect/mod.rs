// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! JWT inspector and passive response checks (wave 18, strand B). The token is read, never
//! verified; its value is shown and never written to a file, history, report, note, lint
//! result or exchange (claim names and deadlines only). The checks look at responses the
//! user already got and send nothing. Interface: types only.
//!
//! Rules of the checks (ids `chk/…`, severities `info` and `warn`) and where a token is
//! looked for are in `CLAUDE.md`. [`checks`] holds the rule mechanism ([`ResponseCheck`],
//! [`run_checks`]) that other passive checks of responses build on; [`tokens`] finds and
//! describes tokens.
//!
//! Where the checks run: after a send on a stored response ([`checks_of`], headers and the first
//! [`BODY_LIMIT`] bytes of the body, whatever the size), in `rt send --checks` on the finished
//! report ([`check_report`]). Rules are off per device (the shell keeps the list) or per
//! collection (`extensions.requesttailor.checks.ignore`, [`ignore_in_collection`]).

mod checks;
mod tokens;

use rt_core::prepare::{PrepareInput, prepare};
use rt_core::send::{ReportBody, SendReport};
use serde::{Deserialize, Serialize};

use crate::workspace::MemFiles;
use crate::{AppError, ResponseStore, SendInput, Workspace};

pub use checks::{BODY_LIMIT, ResponseCheck, ResponseView, builtin, check_response, run_checks};
pub use rt_core::inspect::{Observation, ObservationId, Severity};

/// A token decoded for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JwtInfo {
    pub header: serde_json::Value,
    /// `alg` of the header, if there is one.
    pub algorithm: Option<String>,
    /// The claims, well-known ones first (`iss`, `sub`, `aud`, `iat`, `nbf`, `exp`,
    /// `scope`/`scp`, `azp`), then the others by name.
    pub claims: Vec<JwtClaim>,
    pub warnings: Vec<JwtWarning>,
    /// Unix seconds of `exp`, for the countdown.
    pub expires_at: Option<i64>,
    /// Always `false`: nothing verifies the signature.
    pub signature_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimKind {
    Text,
    /// A Unix time in seconds (`iat`, `nbf`, `exp`, `auth_time`).
    Time,
    List,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JwtClaim {
    pub name: String,
    pub value: serde_json::Value,
    pub kind: ClaimKind,
    /// A short explanation of a well-known claim.
    pub meaning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JwtWarningKind {
    Expired,
    NotYetValid,
    /// Expires in under 30 seconds.
    ExpiresSoon,
    /// `alg: none`.
    AlgorithmNone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtWarning {
    pub kind: JwtWarningKind,
    /// Claim name and deadline, never a value: `token expired 12 min ago`.
    pub text: String,
}

/// Where a token was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TokenLocation {
    /// `Authorization: Bearer …` of the resolved request.
    Authorization,
    /// The request's auth block.
    Auth,
    /// The query parameter `access_token`.
    Query,
    /// A field of a JSON response body (`access_token`, `id_token`, `token`).
    Body { path: String },
    /// A `Set-Cookie` of the response.
    Cookie { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoundToken {
    pub location: TokenLocation,
    pub token: JwtInfo,
}

/// Where a check is switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckScope {
    /// This device (stays out of the repository).
    Device,
    /// The collection (`extensions.requesttailor.checks.ignore` in `opencollection.yml`).
    Collection,
}

/// Request-side input of `InspectApi.tokens`: which request to look at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectTarget {
    /// Request path in the active collection.
    pub path: String,
    #[serde(default)]
    pub environment: Option<String>,
    /// A response to look at as well (`None`: the request only).
    #[serde(default)]
    pub response_id: Option<u64>,
}

/// One rule of the checks, for settings lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleInfo {
    pub id: String,
    pub title: String,
    pub severity: Severity,
}

/// The built-in rules.
pub fn rules() -> Vec<RuleInfo> {
    builtin()
        .iter()
        .map(|c| RuleInfo {
            id: c.id().to_owned(),
            title: c.title().to_owned(),
            severity: c.severity(),
        })
        .collect()
}

/// Reads `token` for display ([`rt_core::jwt::parse`]); never verifies the signature.
pub fn jwt(token: &str) -> Result<JwtInfo, AppError> {
    jwt_at(token, rt_core::jwt::now_secs())
}

/// [`jwt`] at a given time (Unix seconds).
pub fn jwt_at(token: &str, now: i64) -> Result<JwtInfo, AppError> {
    let parsed = rt_core::jwt::parse(token).map_err(|e| AppError::Shell(e.to_string()))?;
    Ok(tokens::describe(&parsed, now))
}

/// The tokens in the resolved request at `path`, as it would go out: variables, environment and
/// secret values of `input`, bearer auth. Nothing is sent. A request that cannot be prepared
/// has no tokens here (the send says why). A token fetched by an OAuth flow at send time is not
/// here: it is fresh.
pub fn request_tokens(
    ws: &Workspace,
    path: &str,
    input: SendInput,
) -> Result<Vec<FoundToken>, AppError> {
    let snapshot = ws.snapshot(path, input)?;
    let context = snapshot
        .collection
        .request(path)
        .ok_or_else(|| AppError::RequestNotFound(path.to_owned()))?;
    let prepare_input = PrepareInput {
        environment: snapshot.environment.as_deref(),
        globals: Vec::new(),
        runtime: snapshot.vars.clone(),
    };
    let Ok(prepared) = prepare(&context, &prepare_input, &MemFiles(&snapshot.files)) else {
        return Ok(Vec::new());
    };
    let from_auth_block = matches!(prepared.auth, rt_core::model::Auth::Bearer { .. });
    Ok(tokens::in_request(
        &prepared.request,
        from_auth_block,
        rt_core::jwt::now_secs(),
    ))
}

/// The first [`BODY_LIMIT`] bytes of a stored response and whether that is all of it.
fn body_prefix(responses: &ResponseStore, id: u64) -> Result<(Vec<u8>, bool), AppError> {
    let report = responses.report(id)?;
    let size = report.response.as_ref().map_or(0, |r| r.size);
    let len = u32::try_from(size.min(BODY_LIMIT as u64)).unwrap_or(u32::MAX);
    Ok((responses.bytes(id, 0, len)?, size <= BODY_LIMIT as u64))
}

/// The tokens in a stored response (JSON fields, `Set-Cookie`).
pub fn response_tokens(responses: &ResponseStore, id: u64) -> Result<Vec<FoundToken>, AppError> {
    let report = responses.report(id)?;
    let Some(response) = &report.response else {
        return Ok(Vec::new());
    };
    let (body, complete) = body_prefix(responses, id)?;
    Ok(tokens::in_response(
        &response.headers,
        &body,
        complete,
        rt_core::jwt::now_secs(),
    ))
}

/// The passive checks of a stored response. `ignore`: rule ids switched off on this device;
/// the collection's own list is added by the caller ([`collection_ignored`]).
pub fn checks_of(
    responses: &ResponseStore,
    id: u64,
    ignore: &[String],
) -> Result<Vec<Observation>, AppError> {
    let report = responses.report(id)?;
    let Some(response) = &report.response else {
        return Ok(Vec::new());
    };
    let (body, complete) = body_prefix(responses, id)?;
    let sent = report.request.as_ref().map_or(&[][..], |r| &r.headers[..]);
    let mut view = ResponseView::new(
        &response.url,
        response.status,
        sent,
        &response.headers,
        &body,
        response.size,
    );
    view.body_complete = complete;
    Ok(check_response(&view, ignore))
}

/// The passive checks of a finished report (`rt send --checks`): the report's body is the
/// whole body up to the send limit; the checks read its first [`BODY_LIMIT`] bytes.
pub fn check_report(report: &SendReport, ignore: &[String]) -> Vec<Observation> {
    let Some(response) = &report.response else {
        return Vec::new();
    };
    let decoded;
    let body: &[u8] = match &response.body {
        ReportBody::Utf8(s) => s.as_bytes(),
        ReportBody::Base64(b) => {
            use base64::Engine as _;
            // Only the part the checks read: 4 characters give 3 bytes.
            let take = (BODY_LIMIT / 3 + 1) * 4;
            let part = &b.as_bytes()[..b.len().min(take)];
            decoded = base64::engine::general_purpose::STANDARD
                .decode(&part[..part.len() / 4 * 4])
                .unwrap_or_default();
            &decoded
        }
    };
    let sent = report.request.as_ref().map_or(&[][..], |r| &r.headers[..]);
    let view = ResponseView::new(
        &response.url,
        response.status,
        sent,
        &response.headers,
        body,
        response.size,
    );
    check_response(&view, ignore)
}

/// The rule ids the open collection switches off (`extensions.requesttailor.checks.ignore`,
/// unsaved edits included).
pub fn collection_ignored(ws: &Workspace) -> Vec<String> {
    ws.doc().checks_config().ignore
}

/// Switches `id` off for the collection (unsaved: `opencollection.yml` is staged like any
/// edit; only the `checks` block changes). Refused for an id the checks do not know.
pub fn ignore_in_collection(ws: &mut Workspace, id: &str) -> Result<(), AppError> {
    if !builtin().iter().any(|c| c.id() == id) {
        return Err(AppError::Shell(format!("unknown check: {id}")));
    }
    let mut config = ws.doc().checks_config();
    if !config.ignore.iter().any(|i| i == id) {
        config.ignore.push(id.to_owned());
        ws.doc_mut().set_checks_config(&config)?;
        ws.refresh();
    }
    Ok(())
}

/// Every rule id that is off, device and collection together.
pub fn all_ignored(ws: &Workspace, device: &[String]) -> Vec<String> {
    let mut all = collection_ignored(ws);
    all.extend(device.iter().cloned());
    all
}
