// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Reporters of `rt run` (wave 7, strand E): live text on stderr ([`text`]), the JSON report
//! ([`json`]) and JUnit XML ([`junit`]). They print the steps as the runner reports them;
//! secret values are already masked there (`send_with`).

pub mod junit;
pub mod text;

use rt_core::runner::{RunStep, RunSummary};
use rt_core::send::SendError;
use serde::Serialize;

pub use junit::junit;

/// What the reporters need besides the steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInfo {
    /// The collection's name.
    pub collection: String,
    /// Iterations of the run; above 1, JUnit suite names carry `[iteration n]`.
    pub iterations: u32,
}

/// Version of the JSON report of `rt run`. Bump on breaking changes.
pub const RUN_REPORT_VERSION: u32 = 1;

#[derive(Serialize)]
struct JsonReport<'a> {
    version: u32,
    summary: &'a RunSummary,
    steps: &'a [RunStep],
}

/// `{"version": 1, "summary": RunSummary, "steps": [RunStep...]}`, indented, with a final
/// newline. Each step carries the report as `rt send` prints it.
pub fn json(summary: &RunSummary, steps: &[RunStep]) -> String {
    let mut out = serde_json::to_string_pretty(&JsonReport {
        version: RUN_REPORT_VERSION,
        summary,
        steps,
    })
    .expect("run report serializes");
    out.push('\n');
    out
}

/// The kind of a request error, as in the report JSON (`transport`, `prepare`, …).
pub fn error_kind(error: &SendError) -> &'static str {
    match error {
        SendError::NotFound(_) => "not-found",
        SendError::Prepare(_) => "prepare",
        SendError::Auth(_) => "auth",
        SendError::Plugin(_) => "plugin",
        SendError::Script(_) => "script",
        SendError::Blocked { .. } => "blocked",
        SendError::Transport(_) => "transport",
    }
}

/// A one-line description of a request error.
pub fn error_message(error: &SendError) -> String {
    match error {
        SendError::NotFound(p) => format!("request not found: {p}"),
        SendError::Prepare(m) => format!("cannot prepare the request: {m}"),
        SendError::Auth(m) => format!("authentication failed: {m}"),
        SendError::Plugin(m) => format!("stopped by a plugin: {m}"),
        SendError::Script(m) => format!("pre-request script failed: {m}"),
        SendError::Blocked { reason, .. } => format!("not sent: {reason}"),
        SendError::Transport(e) => e.to_string(),
    }
}

/// The name of an assertion in reports: `res.status eq 200`.
pub fn assertion_name(a: &rt_core::script::AssertionResult) -> String {
    [
        a.expression.as_str(),
        a.operator.as_str(),
        a.expected.as_str(),
    ]
    .iter()
    .filter(|s| !s.is_empty())
    .copied()
    .collect::<Vec<_>>()
    .join(" ")
}

/// Why an assertion failed.
pub fn assertion_message(a: &rt_core::script::AssertionResult) -> String {
    match (&a.error, &a.actual) {
        (Some(e), _) => e.clone(),
        (None, Some(actual)) => format!("expected {} {}, got {actual}", a.operator, a.expected),
        (None, None) => "the expression did not evaluate".into(),
    }
}
