// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Running one consumer collection against the built provider (wave 19, strand F).

use rt_core::guard::{Effect, effect};
use rt_core::model::Collection;
use rt_core::runner::{RunEvent, RunPlan, RunStep};
use rt_core::script::AssertionResult;
use rt_core::send::{MASK, SendError, SendOptions};
use rt_core::services::Services;
use rt_core::transport::Transport;
use rt_format::FileSet;
use web_time::{Duration, Instant};

use super::{ConsumerFailure, ConsumerRef, ConsumerResult, ConsumerVerifyRequest, MAX_REQUESTS};
use crate::runner::RunSnapshot;

/// Minutes a run may take by default.
pub const DEFAULT_MINUTES: u32 = 10;
/// The most minutes a run may be given.
pub const HARD_MINUTES: u32 = 60;

/// The limits of one `verify` run, shared by all its consumers.
#[derive(Debug)]
pub struct Budget {
    requests_left: usize,
    started: Instant,
    limit: Duration,
}

impl Budget {
    pub fn new(max_minutes: Option<u32>) -> Self {
        let minutes = max_minutes
            .unwrap_or(DEFAULT_MINUTES)
            .clamp(1, HARD_MINUTES);
        Self {
            requests_left: MAX_REQUESTS,
            started: Instant::now(),
            limit: Duration::from_secs(u64::from(minutes) * 60),
        }
    }

    /// The time limit is over.
    pub fn expired(&self) -> bool {
        self.started.elapsed() >= self.limit
    }
}

/// A consumer, found and read: its collection (secret values of its environment filled in by
/// the shell), its files for bodies, and the commit a clone was at.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub collection: Collection,
    pub files: FileSet,
    pub commit: Option<String>,
}

fn cut(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

fn mask(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for s in secrets.iter().filter(|s| !s.is_empty()) {
        out = out.replace(s.as_str(), MASK);
    }
    out
}

fn shown(text: &str, limit: usize, secrets: &[String]) -> String {
    cut(&mask(text, secrets), limit)
}

/// `GET /orders/{id}`: the method and the path as the consumer wrote them, without host,
/// query and fragment; `{{name}}` reads `{name}`.
fn label(collection: &Collection, request_path: &str) -> String {
    let Some(ctx) = collection.request(request_path) else {
        return request_path.to_owned();
    };
    let spec = &ctx.request.spec;
    let mut url = spec.url.trim();
    if let Some(i) = url.find('?') {
        url = &url[..i];
    }
    if let Some(i) = url.find('#') {
        url = &url[..i];
    }
    let mut rest = url;
    if let Some(i) = rest.find("://") {
        rest = rest[i + 3..].find('/').map_or("", |j| &rest[i + 3 + j..]);
    } else if rest.starts_with("{{")
        && let Some(close) = rest.find("}}")
    {
        rest = &rest[close + 2..];
    }
    let path = rest.replace("{{", "{").replace("}}", "}");
    let path = if path.is_empty() {
        "/".to_owned()
    } else {
        path
    };
    format!("{} {}", spec.method.to_ascii_uppercase(), path)
}

fn kind_of(text: &str) -> &'static str {
    let t = text.trim();
    if t == "null" {
        "null"
    } else if t == "true" || t == "false" {
        "boolean"
    } else if t.parse::<f64>().is_ok() {
        "number"
    } else if t.starts_with('{') && t.ends_with('}') {
        "object"
    } else if t.starts_with('[') && t.ends_with(']') {
        "array"
    } else {
        "string"
    }
}

/// What differs, without the response: the type that came back (a status code is shown as
/// it is).
fn difference(a: &AssertionResult, secrets: &[String]) -> String {
    let wanted = format!(
        "expected `{}` {} {}",
        a.expression,
        a.operator,
        cut(&mask(&a.expected, secrets), 60)
    )
    .trim_end()
    .to_owned();
    if let Some(error) = &a.error {
        return format!("{wanted}: {}", shown(error, 160, secrets));
    }
    match &a.actual {
        None => format!("{wanted}, found nothing"),
        Some(actual) if a.expression.to_ascii_lowercase().contains("status") => {
            format!("{wanted}, got {}", shown(actual, 20, secrets))
        }
        Some(actual) => {
            let got = kind_of(actual);
            if !a.expected.is_empty() && got == kind_of(&a.expected) {
                format!("{wanted}, got a different {got}")
            } else {
                format!("{wanted}, got {got}")
            }
        }
    }
}

fn failures_of(
    step: &RunStep,
    collection: &Collection,
    secrets: &[String],
) -> Vec<ConsumerFailure> {
    let request = label(collection, &step.request_path);
    let mut out = Vec::new();
    let mut add = |assertion: String, difference: String| {
        out.push(ConsumerFailure {
            request_path: step.request_path.clone(),
            request: request.clone(),
            assertion,
            difference,
        });
    };
    let report = &step.report;
    if let Some(error) = &report.error {
        let what = if matches!(error, SendError::Blocked { .. }) {
            "send guard"
        } else {
            "request"
        };
        add(
            what.to_owned(),
            shown(&rt_core::stream::error_text(error), 200, secrets),
        );
    }
    for t in report.tests.iter().filter(|t| !t.passed) {
        add(
            format!("test {:?}", shown(&t.name, 80, secrets)),
            t.error
                .as_deref()
                .map_or_else(|| "failed".to_owned(), |e| shown(e, 120, secrets)),
        );
    }
    for a in report.assertions.iter().filter(|a| !a.passed) {
        add(
            shown(
                &format!("{} {} {}", a.expression, a.operator, a.expected),
                100,
                secrets,
            ),
            difference(a, secrets),
        );
    }
    if out.is_empty() && !step.passed && !step.skipped {
        out.push(ConsumerFailure {
            request_path: step.request_path.clone(),
            request,
            assertion: "request".to_owned(),
            difference: "failed".to_owned(),
        });
    }
    out
}

fn is_production(collection: &Collection, environment: &str) -> bool {
    crate::agents::is_production(&collection.config.agents, Some(environment))
}

/// Runs the consumer through the runner, the same as `rt run`, with `baseUrl` of its
/// environment set to the built provider. Failures name the request and the assertion and
/// say how the answer differs, never what it contained. Writing requests are left out
/// unless `request.allow_writes`, and always against a production environment. The send
/// guard of `services` is asked per request, as for any send.
#[allow(clippy::too_many_arguments)]
pub async fn run_consumer<T: Transport>(
    consumer: &ConsumerRef,
    loaded: Loaded,
    request: &ConsumerVerifyRequest,
    budget: &mut Budget,
    transport: &T,
    options: &SendOptions,
    services: &Services<'_>,
    cancelled: &dyn Fn() -> bool,
) -> ConsumerResult {
    let mut result = ConsumerResult {
        name: consumer.name.clone(),
        commit: loaded.commit.clone(),
        requests: 0,
        skipped: 0,
        failures: Vec::new(),
        error: None,
    };
    let fail = |mut result: ConsumerResult, message: String| {
        result.error = Some(message);
        result
    };
    let Some(env) = loaded.collection.environment(&consumer.environment) else {
        return fail(
            result,
            format!(
                "the consumer has no environment {:?} (it has: {})",
                consumer.environment,
                loaded
                    .collection
                    .environments
                    .iter()
                    .map(|e| e.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    };
    let secrets: Vec<String> = env
        .variables
        .iter()
        .filter(|v| v.secret)
        .filter_map(|v| v.value.clone())
        .collect();
    let production = is_production(&loaded.collection, &consumer.environment);

    let all = rt_core::runner::plan(&loaded.collection, None);
    if all.is_empty() {
        return fail(result, "the consumer collection has no requests".into());
    }
    let mut plan = Vec::new();
    for path in all {
        let writes = loaded
            .collection
            .request(&path)
            .is_some_and(|c| effect(&c.request.spec) == Effect::Write);
        if writes && (production || !request.allow_writes) {
            result.skipped += 1;
        } else {
            plan.push(path);
        }
    }
    if plan.len() > budget.requests_left {
        return fail(
            result,
            format!("the run would exceed {MAX_REQUESTS} requests; verify fewer consumers"),
        );
    }
    budget.requests_left -= plan.len();

    let mut vars = vec![("baseUrl".to_owned(), request.base_url.clone())];
    for (name, value) in &consumer.vars {
        let value = value
            .replace("{{provider.baseUrl}}", &request.base_url)
            .replace("{{ provider.baseUrl }}", &request.base_url);
        vars.retain(|(n, _)| n != name);
        vars.push((name.clone(), value));
    }
    let snapshot = RunSnapshot {
        collection: loaded.collection.clone(),
        environment: Some(consumer.environment.clone()),
        vars,
        files: loaded.files,
    };
    let run_plan = RunPlan {
        requests: plan,
        iterations: 1,
        ..Default::default()
    };
    let mut steps: Vec<RunStep> = Vec::new();
    let budget: &Budget = budget;
    snapshot
        .run(
            &run_plan,
            transport,
            options,
            services,
            &mut |event| {
                if let RunEvent::Step { step } = event {
                    steps.push(*step);
                }
            },
            &|| cancelled() || budget.expired(),
        )
        .await;
    for step in &steps {
        if !step.skipped {
            result.requests += 1;
        }
        result
            .failures
            .extend(failures_of(step, &loaded.collection, &secrets));
    }
    if budget.expired() {
        result.error = Some("stopped: the time limit of the run is over".into());
    } else if cancelled() {
        result.error = Some("stopped".into());
    }
    result
}
