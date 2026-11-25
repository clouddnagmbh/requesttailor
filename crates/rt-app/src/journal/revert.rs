// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plans and executes reverts (wave 19, strand C); rules at the module documentation of
//! [`super`].
//!
//! A step is an ordinary request: it goes through `send_with` like any send of the user (guard
//! per step, cookies, tokens, scripts of the collection), is shown in the plan before anything
//! is sent, and leaves an audit entry. Nothing here derives an undoing request: a compensator
//! (an extension) or the user supplies it.

use rt_core::guard::{Effect, effect};
use rt_core::model::{Auth, Collection, Item, KeyValue, RequestEntry, RequestSpec, Variable};
use rt_core::prepare::{PrepareInput, prepare};
use rt_core::send::{MASK, ReportBody, SendError, SendOptions, SendReport, send_with};
use rt_core::services::Services;
use rt_core::transport::Transport;

use super::store::Journal;
use super::{
    Compensation, CompensationAnswer, CompensationItem, CompensationKind, EntryStates,
    JournalEntry, MAX_RETRIES, MAX_STEPS, PluginCompensationRule, PreRead, RevertOptions,
    RevertOutcome, RevertPlan, RevertStep, StepOutcome, StepResult, StepState,
};
use crate::AppError;
use crate::agents::{AuditEntry, AuditOutcome, is_production};
use crate::compare::secret_values;
use crate::plugins::PluginInfo;
use crate::runner::RunSnapshot;
use crate::workspace::MemFiles;

/// The values a manual revert fills into the request the user picked.
pub const VAR_ID: &str = "{{journal.id}}";
pub const VAR_LOCATION: &str = "{{journal.location}}";
pub const VAR_ETAG: &str = "{{journal.etag}}";

/// Largest body a kept state holds.
const STATE_BODY_LIMIT: usize = 256 * 1024;

/// A 64-bit FNV-1a hash of `text` as 16 hex digits.
fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            out.push('{');
            for key in keys {
                out.push_str(&serde_json::to_string(key).unwrap_or_default());
                out.push(':');
                canonical(&map[key], out);
                out.push(',');
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for item in items {
                canonical(item, out);
                out.push(',');
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// The hash the conflict guard compares: of the canonical form of a JSON body (keys sorted, so
/// the order the server writes them in does not matter), or of the text itself when it is not
/// JSON. A [`Compensation::state_hash`] is this hash of the probe's answer right after our
/// write, or `etag:<value>` to compare the `ETag` header instead.
pub fn hash_state(body: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => {
            let mut canon = String::new();
            canonical(&value, &mut canon);
            fnv(&canon)
        }
        Err(_) => fnv(body),
    }
}

fn mask_all(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        let json = serde_json::to_string(secret).unwrap_or_default();
        let json = json.trim_matches('"').to_owned();
        for form in [secret.clone(), json] {
            if !form.is_empty() && out.contains(&form) {
                out = out.replace(&form, MASK);
            }
        }
    }
    out
}

fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Everything a plan or an execution needs besides the plan: the collection as it is now with
/// the secret values of the plan's environment, and how to send.
pub struct RevertEnv<'a, T: Transport> {
    pub snapshot: &'a RunSnapshot,
    pub transport: &'a T,
    pub options: &'a SendOptions,
    pub services: &'a Services<'a>,
    /// Who confirmed in the audit log: `app` or `cli`.
    pub caller: &'a str,
}

/// The collection with the requests of a plan added at the root, so they send like any request
/// (collection defaults, scripts, guard). Paths are `journal-revert/<n>-<role>.yml`.
struct Staged {
    collection: Collection,
}

impl Staged {
    fn new(snapshot: &RunSnapshot) -> Self {
        Self {
            collection: snapshot.collection.clone(),
        }
    }

    fn add(&mut self, tag: &str, spec: &RequestSpec) -> String {
        let path = format!("journal-revert/{tag}.yml");
        self.collection.items.push(Item::Request(RequestEntry {
            path: path.clone(),
            spec: spec.clone(),
        }));
        path
    }
}

fn input_of(snapshot: &RunSnapshot) -> PrepareInput<'_> {
    PrepareInput {
        environment: snapshot.environment.as_deref(),
        globals: Vec::new(),
        runtime: snapshot.vars.clone(),
    }
}

/// The masked, resolved URL of `path` in `collection`; the written URL when preparing fails
/// (it names variables, never values).
fn masked_url(
    snapshot: &RunSnapshot,
    collection: &Collection,
    path: &str,
    spec: &RequestSpec,
) -> String {
    let secrets = secret_values(snapshot);
    let files = MemFiles(&snapshot.files);
    let prepared = collection
        .request(path)
        .and_then(|ctx| prepare(&ctx, &input_of(snapshot), &files).ok());
    match prepared {
        Some(p) => mask_all(&p.request.url, &secrets),
        None => spec.url.clone(),
    }
}

/// `spec` with the defaults of the folders above `path` written into it, so a request the user
/// picked from a folder sends the same from the collection's root.
fn baked(collection: &Collection, path: &str) -> Option<RequestSpec> {
    let ctx = collection.request(path)?;
    let mut spec = ctx.request.spec.clone();
    let mut headers: Vec<KeyValue> = Vec::new();
    for folder in &ctx.folders {
        headers.extend(folder.defaults.headers.iter().cloned());
    }
    headers.extend(spec.headers.iter().cloned());
    let mut merged: Vec<KeyValue> = Vec::new();
    for h in headers.into_iter().rev() {
        if !merged.iter().any(|m| m.name.eq_ignore_ascii_case(&h.name)) {
            merged.push(h);
        }
    }
    merged.reverse();
    spec.headers = merged;
    if matches!(spec.auth, Auth::Inherit) {
        for folder in ctx.folders.iter().rev() {
            if !matches!(folder.defaults.auth, Auth::Inherit) {
                spec.auth = folder.defaults.auth.clone();
                break;
            }
        }
    }
    let mut vars: Vec<Variable> = Vec::new();
    for folder in &ctx.folders {
        vars.extend(folder.defaults.vars.iter().cloned());
    }
    vars.extend(spec.vars.iter().cloned());
    let mut kept: Vec<Variable> = Vec::new();
    for v in vars.into_iter().rev() {
        if !kept.iter().any(|k| k.name == v.name) {
            kept.push(v);
        }
    }
    kept.reverse();
    spec.vars = kept;
    Some(spec)
}

fn replace_in(value: &mut serde_json::Value, from: &str, to: &str) {
    match value {
        serde_json::Value::String(s) if s.contains(from) => *s = s.replace(from, to),
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| replace_in(v, from, to)),
        serde_json::Value::Object(map) => map.values_mut().for_each(|v| replace_in(v, from, to)),
        _ => {}
    }
}

/// `spec` with `{{journal.id}}`, `{{journal.location}}` and `{{journal.etag}}` filled from the
/// entry. A name the request uses that the entry has no value for is an error (a request
/// must not go out with a hole in it).
pub fn fill_entry_values(
    spec: &RequestSpec,
    entry: &JournalEntry,
) -> Result<RequestSpec, AppError> {
    let mut value = serde_json::to_value(spec).map_err(|e| AppError::Shell(e.to_string()))?;
    let text = value.to_string();
    let values = [
        (
            VAR_ID,
            entry.resource_id.as_deref(),
            "an id (set an id path in the journal settings)",
        ),
        (VAR_LOCATION, entry.location.as_deref(), "a Location"),
        (VAR_ETAG, entry.etag.as_deref(), "an ETag"),
    ];
    for (name, found, what) in values {
        if !text.contains(name) {
            continue;
        }
        match found {
            Some(found) => replace_in(&mut value, name, found),
            None => {
                return Err(AppError::Shell(format!(
                    "the request uses {name}, but entry #{} has no {what}",
                    entry.id
                )));
            }
        }
    }
    serde_json::from_value(value).map_err(|e| AppError::Shell(e.to_string()))
}

fn step_of(
    snapshot: &RunSnapshot,
    staged: &mut Staged,
    entry_id: u64,
    compensation: Compensation,
) -> RevertStep {
    let tag = format!("plan-{entry_id}");
    let path = staged.add(&tag, &compensation.request);
    let url = masked_url(snapshot, &staged.collection, &path, &compensation.request);
    RevertStep {
        entry_id,
        state: StepState::Planned,
        method: compensation.request.method.clone(),
        url,
        effect: effect(&compensation.request),
        description: compensation.description.clone(),
        compensation: Some(compensation),
        reason: None,
    }
}

fn blocked_step(entry: &JournalEntry, reason: String) -> RevertStep {
    RevertStep {
        entry_id: entry.id,
        state: StepState::NotReversible,
        method: entry.method.clone(),
        url: entry.url.clone(),
        effect: Effect::Write,
        description: format!("{} {}", entry.method, entry.url),
        compensation: None,
        reason: Some(reason),
    }
}

/// The plan of a manual revert: the request the user picked (as edited in memory), with the
/// entry's values filled in, the folder defaults written into it, shown with its masked
/// URL, its effect and the environment. Nothing is sent.
pub fn plan_manual(
    snapshot: &RunSnapshot,
    entry: &JournalEntry,
    request_path: &str,
) -> Result<RevertPlan, AppError> {
    let spec = baked(&snapshot.collection, request_path)
        .ok_or_else(|| AppError::RequestNotFound(request_path.to_owned()))?;
    let spec = fill_entry_values(&spec, entry)?;
    let compensation = Compensation {
        kind: CompensationKind::Manual,
        description: format!(
            "Send {} with the values of #{} ({} {})",
            if spec.name.is_empty() {
                request_path
            } else {
                &spec.name
            },
            entry.id,
            entry.method,
            entry.url
        ),
        request: spec,
        state_hash: None,
        probe: None,
    };
    let mut staged = Staged::new(snapshot);
    let step = step_of(snapshot, &mut staged, entry.id, compensation);
    Ok(RevertPlan {
        environment: entry.environment.clone(),
        production: is_production(
            &snapshot.collection.config.agents,
            entry.environment.as_deref(),
        ),
        steps: vec![step],
        truncated: false,
    })
}

fn mentions_mask(compensation: &Compensation) -> bool {
    let text = serde_json::to_string(&compensation.request).unwrap_or_default();
    // `***` inside a value is what a masked secret leaves; a JSON string with the mask in it.
    text.contains(MASK)
}

/// The plan for what compensators answered, newest entry first. An entry already reverted,
/// an answer that would write a masked secret back, a probe that is not a reading request:
/// each is a step "not reversible" with its reason. At most [`MAX_STEPS`] steps; the rest are
/// left for another plan (`truncated`).
pub fn plan_compensations(
    snapshot: &RunSnapshot,
    journal: &Journal,
    items: &[CompensationItem],
) -> Result<RevertPlan, AppError> {
    let mut found: Vec<(JournalEntry, &CompensationAnswer)> = Vec::new();
    for item in items {
        let entry = journal.entry(item.entry_id).ok_or_else(|| {
            AppError::Shell(format!("journal entry #{} not found", item.entry_id))
        })?;
        if found.iter().all(|(e, _)| e.id != entry.id) {
            found.push((entry, &item.answer));
        }
    }
    let environment = found.first().and_then(|(e, _)| e.environment.clone());
    if found.iter().any(|(e, _)| e.environment != environment) {
        return Err(AppError::Shell(
            "the entries are from different environments; revert one environment at a time".into(),
        ));
    }
    found.sort_by_key(|(e, _)| std::cmp::Reverse(e.id));
    let truncated = found.len() > MAX_STEPS;
    found.truncate(MAX_STEPS);
    let mut staged = Staged::new(snapshot);
    let mut steps = Vec::new();
    for (entry, answer) in found {
        let step = if entry.reverted_at.is_some() {
            blocked_step(&entry, "already reverted".into())
        } else {
            match answer {
                CompensationAnswer::NotReversible { reason } => {
                    blocked_step(&entry, reason.clone())
                }
                CompensationAnswer::Compensation { compensation } => {
                    let comp = (**compensation).clone();
                    if mentions_mask(&comp) {
                        blocked_step(
                            &entry,
                            "putting it back would need a secret value, which is masked".into(),
                        )
                    } else if comp
                        .probe
                        .as_ref()
                        .is_some_and(|p| effect(p) != Effect::Read)
                    {
                        blocked_step(&entry, "the check request would write".into())
                    } else {
                        step_of(snapshot, &mut staged, entry.id, comp)
                    }
                }
            }
        };
        steps.push(step);
    }
    Ok(RevertPlan {
        production: is_production(&snapshot.collection.config.agents, environment.as_deref()),
        environment,
        steps,
        truncated,
    })
}

/// The compensation rules of the enabled plugins, with the plugin that ships each.
pub fn plugin_rules(plugins: &[PluginInfo]) -> Vec<PluginCompensationRule> {
    plugins
        .iter()
        .filter(|p| p.enabled)
        .flat_map(|p| {
            p.manifest
                .contributes
                .compensations
                .iter()
                .map(|rule| PluginCompensationRule {
                    plugin: p.manifest.id.clone(),
                    rule: rule.clone(),
                })
        })
        .collect()
}

async fn send_staged<T: Transport>(
    env: &RevertEnv<'_, T>,
    staged: &Staged,
    path: &str,
) -> SendReport {
    send_with(
        &staged.collection,
        path,
        &input_of(env.snapshot),
        &MemFiles(&env.snapshot.files),
        env.transport,
        env.options,
        env.services,
    )
    .await
}

fn header_of<'a>(report: &'a SendReport, name: &str) -> Option<&'a str> {
    report
        .response
        .as_ref()?
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn body_of(report: &SendReport) -> String {
    match report.response.as_ref().map(|r| &r.body) {
        Some(ReportBody::Utf8(text)) => text.clone(),
        _ => String::new(),
    }
}

fn status_of(report: &SendReport) -> Option<u16> {
    report.response.as_ref().map(|r| r.status)
}

/// Reads one resource: the request must only read. The answer is kept in memory (masked)
/// as the "before" of the next entry with the same URL.
pub async fn pre_read<T: Transport>(
    env: &RevertEnv<'_, T>,
    journal: &Journal,
    request: &RequestSpec,
) -> Result<PreRead, AppError> {
    if effect(request) != Effect::Read {
        return Err(AppError::Shell(
            "a read before a write must be a reading request".into(),
        ));
    }
    let mut staged = Staged::new(env.snapshot);
    let path = staged.add("pre-read", request);
    let report = send_staged(env, &staged, &path).await;
    let secrets = secret_values(env.snapshot);
    if let Some(error) = &report.error {
        return Err(AppError::Shell(mask_all(&describe(error), &secrets)));
    }
    let status = status_of(&report).unwrap_or(0);
    let state = PreRead {
        status,
        body: Some(mask_all(
            &cut(&body_of(&report), STATE_BODY_LIMIT),
            &secrets,
        )),
        etag: header_of(&report, "etag").map(str::to_owned),
    };
    if let Some(sent) = &report.request {
        journal.keep_pre_read(&sent.url, state.clone());
    }
    Ok(state)
}

fn describe(error: &SendError) -> String {
    use SendError::*;
    match error {
        Blocked { rule, reason } => format!("refused by the send guard ({rule}): {reason}"),
        NotFound(m) | Prepare(m) | Auth(m) | Plugin(m) | Script(m) => m.clone(),
        Transport(t) => t.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn audit_entry(
    env: &RevertEnv<'_, impl Transport>,
    plan: &RevertPlan,
    step: &RevertStep,
    outcome: AuditOutcome,
    status: Option<u16>,
    rule: Option<String>,
    detail: &str,
    approved: bool,
) -> AuditEntry {
    AuditEntry {
        at: super::store::now_ms(),
        agent: env.caller.to_owned(),
        tool: "journal-revert".into(),
        collection: env.snapshot.collection.name.clone(),
        request_path: Some(format!("journal #{}", step.entry_id)),
        environment: plan.environment.clone(),
        method: Some(step.method.clone()),
        url: Some(step.url.clone()),
        status,
        outcome,
        approved_by: approved.then(|| env.caller.to_owned()),
        rule,
        detail: detail.to_owned(),
    }
}

/// Runs the plan: each reversible step in order, newest entry first. Per step: the guard
/// answers (as for any send), the conflict guard reads the current state and skips a resource
/// somebody else changed, `If-Match` is set where the server gave an `ETag`, a 5xx answer is
/// retried up to [`MAX_RETRIES`] times, a read afterwards checks the result, an audit entry is
/// written and the journal entry is marked reverted.
///
/// Errors (nothing sent): the plan is for another environment than the snapshot's, or the
/// environment is production and `options.production_confirmed` is not set.
pub async fn execute<T: Transport>(
    env: &RevertEnv<'_, T>,
    journal: Option<&Journal>,
    plan: &RevertPlan,
    options: &RevertOptions,
    audit: &dyn Fn(&AuditEntry),
    cancelled: &dyn Fn() -> bool,
) -> Result<RevertOutcome, AppError> {
    if plan.environment != env.snapshot.environment {
        return Err(AppError::Shell(
            "the plan is for another environment than the one selected".into(),
        ));
    }
    let production = is_production(
        &env.snapshot.collection.config.agents,
        plan.environment.as_deref(),
    );
    let count = plan
        .steps
        .iter()
        .filter(|s| s.state == StepState::Planned)
        .count();
    if production && count > 0 && !options.production_confirmed {
        return Err(AppError::Shell(format!(
            "Revert {count} change{} in PRODUCTION? It needs the user's explicit confirmation.",
            if count == 1 { "" } else { "s" }
        )));
    }
    let secrets = secret_values(env.snapshot);
    let mut staged = Staged::new(env.snapshot);
    let mut steps = Vec::new();
    for (n, step) in plan.steps.iter().enumerate() {
        let skip = |result: StepResult, message: &str| StepOutcome {
            entry_id: step.entry_id,
            result,
            message: message.to_owned(),
        };
        let compensation = match (&step.state, &step.compensation) {
            (StepState::Planned, Some(c)) => c,
            _ => {
                steps.push(skip(
                    StepResult::SkippedNotReversible,
                    step.reason.as_deref().unwrap_or("not reversible"),
                ));
                continue;
            }
        };
        if cancelled() {
            steps.push(skip(StepResult::Failed, "cancelled"));
            continue;
        }
        let mut request = compensation.request.clone();
        // The conflict guard.
        let mut probe_path = None;
        if let Some(probe) = &compensation.probe {
            let path = staged.add(&format!("{n}-probe"), probe);
            let report = send_staged(env, &staged, &path).await;
            if let Some(error) = &report.error {
                let message = mask_all(
                    &format!("could not read the current state: {}", describe(error)),
                    &secrets,
                );
                audit(&audit_entry(
                    env,
                    plan,
                    step,
                    AuditOutcome::Failed,
                    None,
                    None,
                    &message,
                    production,
                ));
                steps.push(skip(StepResult::Failed, &message));
                continue;
            }
            let status = status_of(&report).unwrap_or(0);
            let etag = header_of(&report, "etag").map(str::to_owned);
            let current = hash_state(&mask_all(&body_of(&report), &secrets));
            if let Some(expected) = &compensation.state_hash {
                let same = match expected.strip_prefix("etag:") {
                    Some(tag) => etag.as_deref() == Some(tag),
                    None => status < 300 && &current == expected,
                };
                if !same {
                    let message = "changed by someone else; left alone".to_owned();
                    audit(&audit_entry(
                        env,
                        plan,
                        step,
                        AuditOutcome::Read,
                        Some(status),
                        None,
                        &message,
                        production,
                    ));
                    steps.push(skip(StepResult::SkippedConflict, &message));
                    continue;
                }
            }
            if let Some(tag) = etag
                && !request
                    .headers
                    .iter()
                    .any(|h| h.name.eq_ignore_ascii_case("if-match"))
            {
                request.headers.push(KeyValue::new("If-Match", tag));
            }
            probe_path = Some(path);
        }
        let path = staged.add(&format!("{n}-step"), &request);
        let mut attempts = 0;
        let (report, rule) = loop {
            let report = send_staged(env, &staged, &path).await;
            attempts += 1;
            let server_error = status_of(&report).is_some_and(|s| s >= 500);
            if report.error.is_some() || !server_error || attempts > MAX_RETRIES {
                let rule = match &report.error {
                    Some(SendError::Blocked { rule, .. }) => Some(rule.clone()),
                    _ => None,
                };
                break (report, rule);
            }
        };
        let status = status_of(&report);
        let (result, message, outcome) = if let Some(error) = &report.error {
            let outcome = if rule.is_some() {
                AuditOutcome::Refused
            } else {
                AuditOutcome::Failed
            };
            (
                StepResult::Failed,
                mask_all(&describe(error), &secrets),
                outcome,
            )
        } else if status.is_some_and(|s| (200..300).contains(&s)) {
            // The check afterwards.
            let mut verdict = (
                StepResult::Reverted,
                format!("HTTP {}", status.unwrap_or(0)),
            );
            if let Some(path) = probe_path.as_ref() {
                let after = send_staged(env, &staged, path).await;
                let seen = status_of(&after);
                let wanted = match compensation.kind {
                    CompensationKind::Delete => seen.is_some_and(|s| s == 404 || s == 410),
                    _ => seen.is_some_and(|s| (200..300).contains(&s)),
                };
                if !wanted {
                    verdict = (
                        StepResult::Failed,
                        format!(
                            "sent, but the check afterwards answered {}",
                            seen.map_or("nothing".to_owned(), |s| s.to_string())
                        ),
                    );
                }
            }
            let outcome = if verdict.0 == StepResult::Reverted {
                AuditOutcome::Sent
            } else {
                AuditOutcome::Failed
            };
            (verdict.0, verdict.1, outcome)
        } else {
            (
                StepResult::Failed,
                format!(
                    "HTTP {}",
                    status.map_or("no answer".to_owned(), |s| s.to_string())
                ),
                AuditOutcome::Failed,
            )
        };
        audit(&audit_entry(
            env, plan, step, outcome, status, rule, &message, production,
        ));
        if result == StepResult::Reverted
            && let Some(journal) = journal
        {
            journal.mark_reverted(step.entry_id);
        }
        steps.push(skip(result, &message));
    }
    Ok(RevertOutcome { steps })
}

/// What the journal keeps in memory for these entries.
pub fn states(journal: &Journal, ids: &[u64]) -> Vec<EntryStates> {
    journal.states(ids)
}
