// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The run of one access check; rules at the module documentation of [`super`].

use std::collections::BTreeSet;

use rt_core::guard::{Effect, effect};
use rt_core::model::{ParamKind, Protocol, RequestSpec, VarScope};
use rt_core::prepare::PrepareInput;
use rt_core::ratelimit::{RateTracker, Throttle, ThrottleMode};
use rt_core::runner::{RunEvent, RunPlan, RunStep, platform_sleep, run_with_tracker};
use rt_core::send::{ReportBody, SendError, SendOptions, SendReport};
use rt_core::services::LocalBoxFuture;
use rt_core::transport::Transport;
use serde_json::Value;

use super::evidence::{cut, evidence, holds_id, looks_like_id, mask_text, object_id, structure};
use super::{
    AccessCheck, AccessFinding, AccessOptions, AccessProfile, AccessVerdict, DEFAULT_RATE,
    EVIDENCE_LIMIT,
};
use crate::agents::{AuditEntry, AuditOutcome, is_production};
use crate::compare::secret_values;
use crate::envcompare::EnvRun;
use crate::runner::RunSnapshot;
use crate::workspace::MemFiles;
use crate::{AppError, SendInput};

/// The longest a rate-limit wait of one probe may be.
const MAX_WAIT_MS: u64 = 30_000;

pub const SKIPPED_WRITES: &str = "skipped (writes)";
pub const SKIPPED_PRODUCTION: &str = "skipped (production)";
pub const NO_REFERENCE: &str = "no object reference found";

/// What `check` works with: the two profiles (a snapshot of the collection per environment,
/// with that environment's secret values, and the services of the send), the transport and
/// who asks (`app`, `cli`) for the audit log.
pub struct AccessEnv<'r, 's, T> {
    pub a: &'r EnvRun<'s>,
    pub b: &'r EnvRun<'s>,
    pub transport: &'r T,
    pub options: &'r SendOptions,
    /// Written to the audit log (`app` or `cli`).
    pub caller: &'r str,
    /// One call per check; the shell appends it to `agents/audit.jsonl` (the web has none).
    pub audit: &'r dyn Fn(&AuditEntry),
    pub cancelled: &'r dyn Fn() -> bool,
}

/// An object A's request pointed at.
#[derive(Debug, Clone)]
struct Reference {
    /// The variable B's run takes it from A for; `None`: it is part of the URL as written.
    variable: Option<String>,
    value: String,
}

/// `{{name}}` tokens of `text`.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        {
            out.push(name.to_owned());
        }
        rest = &after[end + 2..];
    }
    out
}

/// The part of a URL template after the host.
fn after_authority(url: &str) -> &str {
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
    };
    match rest.find(['/', '?']) {
        Some(i) => &rest[i..],
        None => "",
    }
}

/// Variables the request takes object references from: `{{name}}` in the path and query of its
/// URL, and in the values of its path and query parameters.
fn reference_names(spec: &RequestSpec) -> Vec<String> {
    let mut names: Vec<String> = tokens(after_authority(&spec.url));
    for param in spec.params.iter().filter(|p| p.enabled) {
        if matches!(param.kind, ParamKind::Path | ParamKind::Query) {
            names.extend(tokens(&param.value));
        }
    }
    let mut seen = BTreeSet::new();
    names.retain(|n| seen.insert(n.clone()));
    names
}

fn body_text(report: &SendReport) -> Option<&str> {
    match &report.response.as_ref()?.body {
        ReportBody::Utf8(text) => Some(text),
        ReportBody::Base64(_) => None,
    }
}

fn status_of(report: &SendReport) -> Option<u16> {
    report.response.as_ref().map(|r| r.status)
}

fn describe(error: &SendError) -> String {
    use SendError::*;
    match error {
        Blocked { rule, reason } => format!("refused by the send guard ({rule}): {reason}"),
        NotFound(m) | Prepare(m) | Auth(m) | Plugin(m) | Script(m) => m.clone(),
        Transport(t) => t.to_string(),
    }
}

/// One probe: the request as a one-request run (guard, scripts, cookies, plugins and the rate
/// limits of the host are the ones of a normal send).
#[allow(clippy::too_many_arguments)]
async fn probe<T: Transport>(
    env: &AccessEnv<'_, '_, T>,
    run: &EnvRun<'_>,
    snapshot: &RunSnapshot,
    path: &str,
    tracker: &RateTracker,
    sleep: &dyn Fn(u64) -> LocalBoxFuture<'static, ()>,
) -> Option<RunStep> {
    let plan = RunPlan {
        requests: vec![path.to_owned()],
        iterations: 1,
        // Wait for what the host said about its limits; never repeat a request.
        throttle: Throttle {
            mode: ThrottleMode::Respect,
            max_wait_ms: MAX_WAIT_MS,
            retries: 0,
        },
        ..RunPlan::default()
    };
    let input = PrepareInput {
        environment: snapshot.environment.as_deref(),
        globals: Vec::new(),
        runtime: snapshot.vars.clone(),
    };
    let mut steps: Vec<RunStep> = Vec::new();
    run_with_tracker(
        &snapshot.collection,
        &plan,
        &input,
        &MemFiles(&snapshot.files),
        env.transport,
        env.options,
        &run.services,
        &mut |event| {
            if let RunEvent::Step { step } = event {
                steps.push(*step);
            }
        },
        env.cancelled,
        sleep,
        tracker,
    )
    .await;
    steps.into_iter().next()
}

/// The object references of A's answer: variables of the URL (path and query) with the value
/// they had for A, and an id of the answer that is a segment of the URL as sent.
fn references(
    a: &RunSnapshot,
    spec: &RequestSpec,
    path: &str,
    report: &SendReport,
    body: Option<&Value>,
) -> Vec<Reference> {
    let mut out = Vec::new();
    let names = reference_names(spec);
    if !names.is_empty()
        && let Ok(scope) = crate::variables::scope(
            &a.collection,
            path,
            &SendInput {
                environment: a.environment.clone(),
                vars: a.vars.clone(),
                secrets: Vec::new(),
            },
        )
    {
        for name in names {
            // What A's run set (an action reading the id from the answer) wins over what the
            // environment says. Secrets are never copied to the other profile.
            let set = report
                .var_changes
                .iter()
                .rev()
                .find(|c| c.scope == VarScope::Runtime && c.name == name)
                .and_then(|c| c.value.clone())
                .filter(|v| v != rt_core::send::MASK);
            let defined = scope
                .variables
                .iter()
                .rev()
                .find(|v| v.name == name && !v.overridden)
                .filter(|v| !v.secret)
                .and_then(|v| v.value.clone());
            if let Some(value) = set.or(defined).filter(|v| !v.is_empty()) {
                out.push(Reference {
                    variable: Some(name),
                    value,
                });
            }
        }
    }
    if out.is_empty()
        && let Some(id) = body.and_then(object_id)
        && let Some(sent) = &report.request
    {
        let url = sent.url.split(['?', '#']).next().unwrap_or("");
        if url
            .split('/')
            .skip(3)
            .any(|segment| !segment.is_empty() && segment == id)
        {
            out.push(Reference {
                variable: None,
                value: id,
            });
        }
    }
    out
}

fn is_success(status: Option<u16>) -> bool {
    status.is_some_and(|s| (200..300).contains(&s))
}

struct Parts {
    verdict: AccessVerdict,
    findings: Vec<AccessFinding>,
    status_a: Option<u16>,
    status_b: Option<u16>,
    /// For the audit entry.
    url: Option<String>,
    rule: Option<String>,
    outcome: AuditOutcome,
}

impl Parts {
    fn skipped(reason: impl Into<String>, outcome: AuditOutcome) -> Self {
        let reason = reason.into();
        Self {
            findings: vec![AccessFinding {
                verdict: AccessVerdict::Skipped {
                    reason: reason.clone(),
                },
                message: reason.clone(),
                evidence: Vec::new(),
            }],
            verdict: AccessVerdict::Skipped { reason },
            status_a: None,
            status_b: None,
            url: None,
            rule: None,
            outcome,
        }
    }
}

fn finding(
    verdict: AccessVerdict,
    message: String,
    evidence: Vec<super::AccessEvidence>,
) -> AccessFinding {
    AccessFinding {
        verdict,
        message,
        evidence,
    }
}

/// Everything secret of both profiles, longest first.
fn all_secrets(a: &RunSnapshot, b: &RunSnapshot) -> Vec<String> {
    let mut all: Vec<String> = secret_values(a)
        .into_iter()
        .chain(secret_values(b))
        .collect();
    all.sort_by_key(|s| std::cmp::Reverse(s.len()));
    all.dedup();
    all
}

/// How B's answer compares with A's object.
fn matches_object(
    a: Option<&Value>,
    a_text: &str,
    b: Option<&Value>,
    b_text: &str,
    id: Option<&str>,
) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => id.is_some_and(|id| holds_id(b, id)) || structure(a) == structure(b),
        (None, None) => a_text.trim() == b_text.trim(),
        _ => false,
    }
}

/// [`check`] with the caller's tracker and sleep (tests give a virtual clock).
pub async fn check_with<T: Transport>(
    env: &AccessEnv<'_, '_, T>,
    path: &str,
    profile: &AccessProfile,
    options: &AccessOptions,
    tracker: &RateTracker,
    sleep: &dyn Fn(u64) -> LocalBoxFuture<'static, ()>,
) -> Result<AccessCheck, AppError> {
    if profile.a == profile.b {
        return Err(AppError::Shell(
            "the access check needs two different environments".into(),
        ));
    }
    for (name, run) in [(&profile.a, env.a), (&profile.b, env.b)] {
        if run.snapshot.collection.environment(name).is_none() {
            return Err(AppError::EnvironmentNotFound(name.clone()));
        }
    }
    let spec = env
        .a
        .snapshot
        .collection
        .request(path)
        .ok_or_else(|| AppError::RequestNotFound(path.to_owned()))?
        .request
        .spec
        .clone();
    let secrets = all_secrets(&env.a.snapshot, &env.b.snapshot);
    let parts = run_check(env, path, profile, options, &spec, &secrets, tracker, sleep).await;

    let entry = AuditEntry {
        at: audit_now(),
        agent: env.caller.to_owned(),
        tool: "access-check".into(),
        collection: env.a.snapshot.collection.name.clone(),
        request_path: Some(path.to_owned()),
        environment: Some(format!("{} vs {}", profile.a, profile.b)),
        method: Some(spec.method.clone()),
        url: parts.url.clone().map(|u| mask_text(&u, &secrets)),
        status: parts.status_b.or(parts.status_a),
        outcome: parts.outcome,
        approved_by: (options.allow_writes && effect(&spec) == Effect::Write)
            .then(|| env.caller.to_owned()),
        rule: parts.rule.clone(),
        detail: mask_text(
            &format!(
                "{}{}",
                verdict_name(&parts.verdict),
                if options.allow_writes && effect(&spec) == Effect::Write {
                    "; writes allowed for this check"
                } else {
                    ""
                }
            ),
            &secrets,
        ),
    };
    (env.audit)(&entry);
    Ok(AccessCheck {
        path: path.to_owned(),
        verdict: parts.verdict,
        findings: parts.findings,
        status_a: parts.status_a,
        status_b: parts.status_b,
    })
}

fn audit_now() -> i64 {
    rt_core::ratelimit::unix_now_ms()
}

fn verdict_name(verdict: &AccessVerdict) -> String {
    match verdict {
        AccessVerdict::Ok => "ok".into(),
        AccessVerdict::BrokenObjectAccess => "broken-object-access".into(),
        AccessVerdict::BrokenFunctionAccess => "broken-function-access".into(),
        AccessVerdict::Inconclusive => "inconclusive".into(),
        AccessVerdict::Skipped { reason } => format!("skipped: {reason}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_check<T: Transport>(
    env: &AccessEnv<'_, '_, T>,
    path: &str,
    profile: &AccessProfile,
    options: &AccessOptions,
    spec: &RequestSpec,
    secrets: &[String],
    tracker: &RateTracker,
    sleep: &dyn Fn(u64) -> LocalBoxFuture<'static, ()>,
) -> Parts {
    if !matches!(spec.protocol, Protocol::Http) {
        return Parts::skipped("only HTTP requests are checked", AuditOutcome::Read);
    }
    if effect(spec) == Effect::Write {
        let policy = &env.a.snapshot.collection.config.agents;
        if is_production(policy, Some(&profile.a)) || is_production(policy, Some(&profile.b)) {
            return Parts::skipped(SKIPPED_PRODUCTION, AuditOutcome::Denied);
        }
        if !options.allow_writes {
            return Parts::skipped(SKIPPED_WRITES, AuditOutcome::Denied);
        }
    }
    let gap = 1000
        / u64::from(if options.rate == 0 {
            DEFAULT_RATE
        } else {
            options.rate
        });

    // A, the reference.
    let started = tracker.now();
    let Some(step_a) = probe(env, env.a, &env.a.snapshot, path, tracker, sleep).await else {
        return Parts::skipped("cancelled", AuditOutcome::Failed);
    };
    let report_a = step_a.report;
    let status_a = status_of(&report_a);
    let url = report_a.request.as_ref().map(|r| r.url.clone());
    let parts = |verdict, findings, status_b, outcome| Parts {
        verdict,
        findings,
        status_a,
        status_b,
        url: url.clone(),
        rule: None,
        outcome,
    };
    if let Some(SendError::Blocked { rule, reason }) = &report_a.error {
        let mut p = Parts::skipped(
            format!("refused by the send guard ({rule}): {reason}"),
            AuditOutcome::Refused,
        );
        p.rule = Some(rule.clone());
        return p;
    }
    let a_text = body_text(&report_a).unwrap_or("").to_owned();
    let evidence_a = |report: &SendReport| {
        evidence(
            "A",
            status_of(report),
            report.response.as_ref().map_or(&[][..], |r| &r.headers[..]),
            body_text(report),
            secrets,
        )
    };
    if !is_success(status_a) {
        let why = match (&report_a.error, status_a) {
            (Some(e), _) => format!("A's request failed: {}", mask_text(&describe(e), secrets)),
            (None, Some(s)) => format!("A got status {s}: there is no object to compare"),
            _ => "A got no answer".into(),
        };
        return parts(
            AccessVerdict::Inconclusive,
            vec![finding(
                AccessVerdict::Inconclusive,
                why,
                vec![evidence_a(&report_a)],
            )],
            None,
            AuditOutcome::Sent,
        );
    }
    let json_a: Option<Value> = serde_json::from_str(&a_text).ok();
    let refs = references(&env.a.snapshot, spec, path, &report_a, json_a.as_ref());
    if refs.is_empty() && !options.b_must_be_denied {
        let mut p = Parts::skipped(NO_REFERENCE, AuditOutcome::Sent);
        p.status_a = status_a;
        p.url = url;
        p.findings[0].evidence = vec![evidence_a(&report_a)];
        return p;
    }
    if (env.cancelled)() {
        return Parts::skipped("cancelled", AuditOutcome::Failed);
    }

    // B, with the ids A saw as variables. No more than the rate says.
    let spent = tracker.now().saturating_sub(started);
    if gap > spent {
        sleep(gap - spent).await;
    }
    let mut snapshot_b = env.b.snapshot.clone();
    for r in &refs {
        if let Some(name) = &r.variable {
            snapshot_b.vars.retain(|(n, _)| n != name);
            snapshot_b.vars.push((name.clone(), r.value.clone()));
        }
    }
    let Some(step_b) = probe(env, env.b, &snapshot_b, path, tracker, sleep).await else {
        return Parts::skipped("cancelled", AuditOutcome::Failed);
    };
    let report_b = step_b.report;
    let status_b = status_of(&report_b);
    let evidence_b = |report: &SendReport| {
        evidence(
            "B",
            status_of(report),
            report.response.as_ref().map_or(&[][..], |r| &r.headers[..]),
            body_text(report),
            secrets,
        )
    };
    let both = |a: &SendReport, b: &SendReport| vec![evidence_a(a), evidence_b(b)];
    if let Some(SendError::Blocked { rule, reason }) = &report_b.error {
        let mut p = parts(
            AccessVerdict::Skipped {
                reason: format!("refused by the send guard ({rule}): {reason}"),
            },
            Vec::new(),
            None,
            AuditOutcome::Refused,
        );
        p.rule = Some(rule.clone());
        p.findings = vec![finding(
            p.verdict.clone(),
            format!("B's request was refused by the send guard ({rule})"),
            vec![evidence_a(&report_a)],
        )];
        return p;
    }
    let b_text = body_text(&report_b).unwrap_or("").to_owned();
    let json_b: Option<Value> = serde_json::from_str(&b_text).ok();
    let id = refs
        .first()
        .map(|r| r.value.clone())
        .or_else(|| json_a.as_ref().and_then(object_id));
    let id_shown = id.as_deref().filter(|i| looks_like_id(i));

    if let Some(error) = &report_b.error {
        let why = format!(
            "B's request failed: {}",
            mask_text(&describe(error), secrets)
        );
        return parts(
            AccessVerdict::Inconclusive,
            vec![finding(
                AccessVerdict::Inconclusive,
                why,
                both(&report_a, &report_b),
            )],
            status_b,
            AuditOutcome::Sent,
        );
    }
    if matches!(status_b, Some(401 | 403 | 404)) {
        let s = status_b.unwrap_or_default();
        return parts(
            AccessVerdict::Ok,
            vec![finding(
                AccessVerdict::Ok,
                format!("B was denied (status {s})"),
                both(&report_a, &report_b),
            )],
            status_b,
            AuditOutcome::Sent,
        );
    }
    if !is_success(status_b) {
        let why = match status_b {
            Some(s) => format!("B got status {s}, which says neither allowed nor denied"),
            None => "B got no answer".into(),
        };
        return parts(
            AccessVerdict::Inconclusive,
            vec![finding(
                AccessVerdict::Inconclusive,
                why,
                both(&report_a, &report_b),
            )],
            status_b,
            AuditOutcome::Sent,
        );
    }

    // B got a success answer.
    let mut findings = Vec::new();
    let object = !refs.is_empty()
        && matches_object(
            json_a.as_ref(),
            &a_text,
            json_b.as_ref(),
            &b_text,
            id.as_deref(),
        );
    if object {
        let message = match id_shown {
            Some(id) => format!("B received object {id} that A's request returned"),
            None => "B received an object that A's request returned".into(),
        };
        findings.push(finding(
            AccessVerdict::BrokenObjectAccess,
            message,
            both(&report_a, &report_b),
        ));
    }
    if options.b_must_be_denied {
        findings.push(finding(
            AccessVerdict::BrokenFunctionAccess,
            format!(
                "B must be denied on {} but got status {}",
                cut(&mask_text(&spec.method, secrets), 16),
                status_b.unwrap_or_default()
            ),
            both(&report_a, &report_b),
        ));
    }
    if findings.is_empty() {
        let why = "B got a success answer that does not look like A's object".to_owned();
        return parts(
            AccessVerdict::Inconclusive,
            vec![finding(
                AccessVerdict::Inconclusive,
                why,
                both(&report_a, &report_b),
            )],
            status_b,
            AuditOutcome::Sent,
        );
    }
    let verdict = if object {
        AccessVerdict::BrokenObjectAccess
    } else {
        AccessVerdict::BrokenFunctionAccess
    };
    // Messages can carry nothing secret, but they are bounded all the same.
    for f in &mut findings {
        f.message = cut(&mask_text(&f.message, secrets), EVIDENCE_LIMIT);
    }
    parts(verdict, findings, status_b, AuditOutcome::Sent)
}

/// Runs the access check of one request (module documentation of [`super`]).
///
/// Errors: the two environments are the same or unknown, or the request does not exist. A
/// refusal of the send guard, a writing request without permission and a production
/// environment are verdicts ([`AccessVerdict::Skipped`]), not errors.
pub async fn check<T: Transport>(
    env: &AccessEnv<'_, '_, T>,
    path: &str,
    profile: &AccessProfile,
    options: &AccessOptions,
) -> Result<AccessCheck, AppError> {
    check_with(
        env,
        path,
        profile,
        options,
        &RateTracker::new(),
        &platform_sleep,
    )
    .await
}
