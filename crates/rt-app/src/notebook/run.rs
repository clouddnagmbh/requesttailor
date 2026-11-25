// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Running the request blocks of a notebook; rules in the module documentation of [`super`].

use rt_core::guard::{Effect, effect};
use rt_core::model::{Collection, Item, Protocol, RequestEntry, RequestSpec};
use rt_core::prepare::PrepareInput;
use rt_core::ratelimit::{RateTracker, Throttle, ThrottleMode};
use rt_core::runner::{RunEvent, RunPlan, RunStep, platform_sleep, run_with_tracker};
use rt_core::send::{MASK, ReportBody, SendError, SendOptions, SendReport};
use rt_core::services::LocalBoxFuture;
use rt_core::transport::Transport;
use serde_json::Value;

use super::{Block, BlockResult, BlockView, Notebook, NotebookOutcome, NotebookRun, Show};
use crate::AppError;
use crate::agents::{AuditEntry, AuditOutcome, is_production};
use crate::compare::secret_values;
use crate::envcompare::EnvRun;
use crate::runner::RunSnapshot;
use crate::views::{ResponseView, ViewDisplay, ViewLanguage};
use crate::workspace::MemFiles;

pub const SKIPPED_WRITES: &str = "skipped (writes)";
pub const SKIPPED_PRODUCTION: &str = "skipped (production)";
pub const SKIPPED_EARLIER: &str = "skipped (an earlier block failed)";

/// The longest a rate-limit wait of one block may be.
const MAX_WAIT_MS: u64 = 30_000;
/// The longest value of a `set:` variable shown in a result.
const SET_LIMIT: usize = 4096;

/// What `run` works with: the collection snapshot of the selected environment with that
/// environment's secret values and the services of the send, the transport and who asks
/// (`app`, `cli`) for the audit log.
pub struct NotebookEnv<'r, 's, T> {
    pub run: &'r EnvRun<'s>,
    pub transport: &'r T,
    pub options: &'r SendOptions,
    /// Written to the audit log (`app` or `cli`).
    pub caller: &'r str,
    /// One call per run; the shell appends it to `agents/audit.jsonl` (the web has none).
    pub audit: &'r dyn Fn(&AuditEntry),
    pub cancelled: &'r dyn Fn() -> bool,
}

fn is_credential_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "key",
        "auth",
        "credential",
        "session",
    ]
    .iter()
    .any(|w| n.contains(w))
}

/// `text` with every secret (also in its JSON-escaped forms) replaced by the mask.
fn mask_text(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        let json = serde_json::to_string(secret).unwrap_or_default();
        let json = json.trim_matches('"').to_owned();
        for form in [secret.clone(), json, secret.escape_debug().to_string()] {
            if !form.is_empty() && out.contains(&form) {
                out = out.replace(&form, MASK);
            }
        }
    }
    out
}

fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn mask_report(report: &mut SendReport, secrets: &[String]) {
    if secrets.is_empty() {
        return;
    }
    if let Some(r) = &mut report.request {
        r.url = mask_text(&r.url, secrets);
        for (_, v) in &mut r.headers {
            *v = mask_text(v, secrets);
        }
    }
    if let Some(r) = &mut report.response {
        r.url = mask_text(&r.url, secrets);
        for (_, v) in &mut r.headers {
            *v = mask_text(v, secrets);
        }
        if let ReportBody::Utf8(text) = &mut r.body {
            *text = mask_text(text, secrets);
        }
    }
    for note in &mut report.notes {
        note.text = mask_text(&note.text, secrets);
    }
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn body_json(report: &SendReport) -> Option<Value> {
    match &report.response.as_ref()?.body {
        ReportBody::Utf8(text) => serde_json::from_str(text).ok(),
        ReportBody::Base64(_) => None,
    }
}

fn describe(error: &SendError) -> String {
    use SendError::*;
    match error {
        Blocked { rule, reason } => format!("refused by the send guard ({rule}): {reason}"),
        NotFound(m) | Prepare(m) | Auth(m) | Plugin(m) | Script(m) => m.clone(),
        Transport(t) => t.to_string(),
    }
}

fn skipped(block: &str, why: impl Into<String>) -> BlockResult {
    BlockResult {
        block: block.to_owned(),
        report: None,
        set: Vec::new(),
        skipped: Some(why.into()),
        views: Vec::new(),
        notes: Vec::new(),
    }
}

/// Runs one request as a one-request run: guard, scripts, cookies, plugins and the rate
/// limits of the host are the ones of a normal send.
async fn probe<T: Transport>(
    env: &NotebookEnv<'_, '_, T>,
    collection: &Collection,
    snapshot: &RunSnapshot,
    vars: &[(String, String)],
    path: &str,
    tracker: &RateTracker,
    sleep: &dyn Fn(u64) -> LocalBoxFuture<'static, ()>,
) -> Option<RunStep> {
    let plan = RunPlan {
        requests: vec![path.to_owned()],
        iterations: 1,
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
        runtime: vars.to_vec(),
    };
    let mut steps: Vec<RunStep> = Vec::new();
    run_with_tracker(
        collection,
        &plan,
        &input,
        &MemFiles(&snapshot.files),
        env.transport,
        env.options,
        &env.run.services,
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

fn view_of(show: &Show, body: &[u8]) -> Option<BlockView> {
    let (path, display) = match show {
        Show::Response => return None,
        Show::Table { path } => (path, ViewDisplay::Table),
        Show::Chart { path } => (path, ViewDisplay::Json),
    };
    let view = ResponseView {
        name: String::new(),
        language: ViewLanguage::Jsonpath,
        expression: path.clone(),
        display,
        columns: Vec::new(),
    };
    let result =
        crate::views::apply_body(body, &view).unwrap_or_else(|e| crate::views::ViewResult {
            display,
            values: Vec::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            notice: Some(e.to_string()),
        });
    Some(BlockView {
        show: show.clone(),
        result,
    })
}

/// [`run`] with the caller's tracker and sleep (tests give a virtual clock).
pub async fn run_with<T: Transport>(
    env: &NotebookEnv<'_, '_, T>,
    notebook: &Notebook,
    request: &NotebookRun,
    tracker: &RateTracker,
    sleep: &dyn Fn(u64) -> LocalBoxFuture<'static, ()>,
) -> Result<NotebookOutcome, AppError> {
    let snapshot = &env.run.snapshot;
    let blocks: Vec<&Block> = notebook
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Request { .. }))
        .collect();
    if blocks.len() > super::MAX_BLOCKS {
        return Err(AppError::Shell(format!(
            "a notebook runs at most {} request blocks",
            super::MAX_BLOCKS
        )));
    }
    if let Some(id) = &request.block
        && !blocks
            .iter()
            .any(|b| matches!(b, Block::Request { id: i, .. } if i == id))
    {
        return Err(AppError::Shell(format!(
            "{} has no block {id:?}",
            request.path
        )));
    }
    if let Some(name) = &snapshot.environment
        && snapshot.collection.environment(name).is_none()
    {
        return Err(AppError::EnvironmentNotFound(name.clone()));
    }

    // Embedded requests live in a copy of the collection under a path no file has.
    let mut collection = snapshot.collection.clone();
    for block in &blocks {
        if let Block::Request {
            id,
            embedded: Some(spec),
            ..
        } = block
        {
            let mut spec = spec.clone();
            spec.name = id.clone();
            collection.items.push(Item::Request(RequestEntry {
                path: format!("{}#{id}", request.path),
                spec,
            }));
        }
    }

    let mut hidden: Vec<String> = Vec::new();
    let mut secrets: Vec<String> = secret_values(snapshot);
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    secrets.dedup();
    // The variables of the run: the caller's, then what earlier blocks set.
    let mut vars: Vec<(String, String)> = snapshot.vars.clone();
    let put = |vars: &mut Vec<(String, String)>, name: &str, value: &str| {
        vars.retain(|(n, _)| n != name);
        vars.push((name.to_owned(), value.to_owned()));
    };
    for (name, value) in &request.vars {
        if value != MASK {
            put(&mut vars, name, value);
        }
    }

    let policy = &snapshot.collection.config.agents;
    let production = is_production(policy, snapshot.environment.as_deref());
    let mut results: Vec<BlockResult> = Vec::new();
    let mut failed = false;
    let (mut sent, mut denied, mut wrote) = (0u32, 0u32, false);
    let mut last_url: Option<String> = None;
    let mut last_status: Option<u16> = None;
    let mut refused_rule: Option<String> = None;

    for block in blocks {
        let Block::Request {
            id,
            request_ref,
            embedded,
            set,
            show,
        } = block
        else {
            continue;
        };
        if request.block.as_ref().is_some_and(|only| only != id) {
            continue;
        }
        if (env.cancelled)() {
            break;
        }
        if failed {
            results.push(skipped(id, SKIPPED_EARLIER));
            continue;
        }
        let path = match (request_ref, embedded) {
            (Some(p), _) => p.clone(),
            (None, Some(_)) => format!("{}#{id}", request.path),
            _ => {
                results.push(skipped(id, "the block names no request"));
                continue;
            }
        };
        let Some(ctx) = collection.request(&path) else {
            results.push(skipped(id, format!("no request {path} in the collection")));
            failed = true;
            continue;
        };
        let spec: &RequestSpec = &ctx.request.spec;
        if !matches!(spec.protocol, Protocol::Http) {
            results.push(skipped(id, "only HTTP requests run in a notebook"));
            continue;
        }
        if effect(spec) == Effect::Write {
            if production {
                denied += 1;
                results.push(skipped(id, SKIPPED_PRODUCTION));
                continue;
            }
            if !request.allow_writes {
                denied += 1;
                results.push(skipped(id, SKIPPED_WRITES));
                continue;
            }
            wrote = true;
        }
        let Some(step) = probe(env, &collection, snapshot, &vars, &path, tracker, sleep).await
        else {
            break;
        };
        sent += 1;
        let mut report = step.report;
        let mut all = secrets.clone();
        all.extend(hidden.iter().cloned());
        all.sort_by_key(|s| std::cmp::Reverse(s.len()));
        all.dedup();
        mask_report(&mut report, &all);
        last_url = report.request.as_ref().map(|r| r.url.clone());
        last_status = report.response.as_ref().map(|r| r.status);
        if let Some(SendError::Blocked { rule, .. }) = &report.error {
            refused_rule = Some(rule.clone());
        }
        if report.error.is_some() {
            failed = true;
        }
        let mut result = BlockResult {
            block: id.clone(),
            report: None,
            set: Vec::new(),
            skipped: None,
            views: Vec::new(),
            notes: Vec::new(),
        };
        if let Some(error) = &report.error {
            result.notes.push(mask_text(&describe(error), &all));
        }
        // Variables from the response. A value that is a credential is carried and not shown.
        if !set.is_empty() {
            match body_json(&report) {
                None => result
                    .notes
                    .push("the response is not JSON; no variable was set".into()),
                Some(json) => {
                    for (name, expression) in set {
                        let found = serde_json_path::JsonPath::parse(expression)
                            .map_err(|e| format!("{name}: invalid JSONPath {expression:?}: {e}"))
                            .map(|p| p.query(&json).first().map(text_of));
                        match found {
                            Err(note) => result.notes.push(note),
                            Ok(None) => result.notes.push(format!(
                                "{name}: {expression} matched nothing; the variable is not set"
                            )),
                            Ok(Some(value)) if value == MASK => result
                                .notes
                                .push(format!("{name}: the value is secret and was not carried")),
                            Ok(Some(value)) => {
                                put(&mut vars, name, &value);
                                let shown = if is_credential_name(name) {
                                    if !value.is_empty() {
                                        hidden.push(value.clone());
                                    }
                                    MASK.to_owned()
                                } else {
                                    cut(&mask_text(&value, &all), SET_LIMIT)
                                };
                                result.set.push((name.clone(), shown));
                            }
                        }
                    }
                }
            }
        }
        if let ReportBody::Utf8(body) = report
            .response
            .as_ref()
            .map(|r| &r.body)
            .unwrap_or(&ReportBody::Utf8(String::new()))
        {
            for s in show {
                if let Some(view) = view_of(s, body.as_bytes()) {
                    result.views.push(view);
                }
            }
        }
        // Later reports hide a credential that was set; this one too.
        let mut again = hidden.clone();
        again.sort_by_key(|s| std::cmp::Reverse(s.len()));
        mask_report(&mut report, &again);
        for view in &mut result.views {
            for value in &mut view.result.values {
                if let Ok(mut text) = serde_json::to_string(value) {
                    let masked = mask_text(&text, &again);
                    if masked != text {
                        text = masked;
                        *value = serde_json::from_str(&text).unwrap_or(Value::String(text));
                    }
                }
            }
        }
        result.report = Some(report);
        results.push(result);
    }

    let mut all = secrets;
    all.extend(hidden);
    let outcome = if refused_rule.is_some() {
        AuditOutcome::Refused
    } else if sent == 0 && denied > 0 {
        AuditOutcome::Denied
    } else if failed {
        AuditOutcome::Failed
    } else {
        AuditOutcome::Sent
    };
    (env.audit)(&AuditEntry {
        at: rt_core::ratelimit::unix_now_ms(),
        agent: env.caller.to_owned(),
        tool: "notebook-run".into(),
        collection: snapshot.collection.name.clone(),
        request_path: Some(request.path.clone()),
        environment: snapshot.environment.clone(),
        method: None,
        url: last_url.map(|u| mask_text(&u, &all)),
        status: last_status,
        outcome,
        approved_by: (wrote && request.allow_writes).then(|| env.caller.to_owned()),
        rule: refused_rule,
        detail: mask_text(
            &format!(
                "{sent} sent, {denied} skipped{}",
                if wrote {
                    "; writes allowed for this run"
                } else {
                    ""
                }
            ),
            &all,
        ),
    });
    Ok(NotebookOutcome { results })
}

/// Runs the request blocks of `notebook` (all of them, or `request.block`) in order, passing
/// variables from block to block (module documentation of [`super`]).
///
/// Errors: an unknown environment or block id, too many blocks. A request the guard
/// refuses, a writing block without permission and a production environment are results
/// (`skipped`), not errors; after a block that failed to send the later ones are skipped.
pub async fn run<T: Transport>(
    env: &NotebookEnv<'_, '_, T>,
    notebook: &Notebook,
    request: &NotebookRun,
) -> Result<NotebookOutcome, AppError> {
    run_with(env, notebook, request, &RateTracker::new(), &platform_sleep).await
}
