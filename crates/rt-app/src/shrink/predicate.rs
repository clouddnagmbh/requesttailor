// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! "Still fails": evaluating a [`Predicate`] on the report of one probe, and turning the
//! failure of an answer into a predicate (`LastFailure`). Bodies are only looked at, never
//! copied into any output.

use base64::Engine as _;
use rt_core::send::{ReportBody, SendReport};

use super::Predicate;

/// The response body as text (a binary body is read lossily).
fn body_text(report: &SendReport) -> String {
    match report.response.as_ref().map(|r| &r.body) {
        Some(ReportBody::Utf8(text)) => text.clone(),
        Some(ReportBody::Base64(data)) => base64::engine::general_purpose::STANDARD
            .decode(data)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default(),
        None => String::new(),
    }
}

/// Whether the failed assertion `which` (an index into the report's assertions, or an
/// expression) failed; `None`: any assertion or `test(...)` failed.
fn assertion_failed(report: &SendReport, which: Option<&str>) -> bool {
    let Some(which) = which else {
        return report.assertions.iter().any(|a| !a.passed)
            || report.tests.iter().any(|t| !t.passed);
    };
    if let Ok(index) = which.parse::<usize>() {
        return report.assertions.get(index).is_some_and(|a| !a.passed);
    }
    report
        .assertions
        .iter()
        .any(|a| !a.passed && a.expression == which)
}

/// Whether `predicate` holds for `report`. `LastFailure` must be resolved first
/// ([`resolve`]); on its own it never holds.
pub fn holds(predicate: &Predicate, report: &SendReport) -> bool {
    let status = report.response.as_ref().map(|r| r.status);
    match predicate {
        Predicate::Status { status: wanted } => status == Some(*wanted),
        Predicate::ServerError => status.is_some_and(|s| (500..600).contains(&s)),
        Predicate::AssertionFails { assertion } => assertion_failed(report, assertion.as_deref()),
        Predicate::BodyMatches { pattern } => {
            report.response.is_some()
                && regex_lite::Regex::new(pattern).is_ok_and(|re| re.is_match(&body_text(report)))
        }
        Predicate::HeaderPresent { name } => report
            .response
            .as_ref()
            .is_some_and(|r| r.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))),
        Predicate::HeaderMissing { name } => report
            .response
            .as_ref()
            .is_some_and(|r| !r.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))),
        Predicate::LastFailure => false,
    }
}

/// What failed in this answer, as predicates that must all hold again: its status (when it is
/// an error status) and the first failed assertion (or "some test failed"). Empty when the
/// answer did not fail.
pub fn failure_of(report: &SendReport) -> Vec<Predicate> {
    let mut out = Vec::new();
    if let Some(status) = report.response.as_ref().map(|r| r.status)
        && status >= 400
    {
        out.push(Predicate::Status { status });
    }
    if let Some(index) = report.assertions.iter().position(|a| !a.passed) {
        out.push(Predicate::AssertionFails {
            assertion: Some(index.to_string()),
        });
    } else if report.tests.iter().any(|t| !t.passed) {
        out.push(Predicate::AssertionFails { assertion: None });
    }
    out
}

/// The single most telling predicate for the failure of `report` (for the dialog; the search
/// itself keeps both the status and the assertion): a failed assertion first, else the
/// error status. `None`: the answer did not fail.
pub fn describe(report: &SendReport) -> Option<Predicate> {
    let mut all = failure_of(report);
    match all.len() {
        0 => None,
        1 => all.pop(),
        _ => all.pop(),
    }
}

/// Checks that a regular expression of [`Predicate::BodyMatches`] compiles.
pub fn validate(predicate: &Predicate) -> Result<(), String> {
    if let Predicate::BodyMatches { pattern } = predicate {
        regex_lite::Regex::new(pattern).map_err(|e| format!("the pattern is not valid: {e}"))?;
    }
    Ok(())
}
