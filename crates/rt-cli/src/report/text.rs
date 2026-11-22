// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The live text of `rt run` on stderr: one line per request as it finishes, failed tests,
//! assertions, script output and warnings indented below it, a summary at the end.
//!
//! ```text
//! ✓ GET List users  200  12 ms
//! ✗ POST Create user  201  30 ms
//!     ✗ name is set: expected undefined to equal 'Ann'
//!     > created 17
//! - DELETE Cleanup  skipped
//! ```

use std::fmt::Write as _;

use rt_core::runner::{RunStep, RunSummary};
use rt_core::services::NoteLevel;

use super::{RunInfo, assertion_message, assertion_name, error_message};

/// The first line of the run.
pub fn started(info: &RunInfo, requests: usize) -> String {
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let mut out = format!(
        "rt run: {}, {}",
        info.collection,
        plural(requests, "request")
    );
    if info.iterations > 1 {
        let _ = write!(out, " × {} iterations", info.iterations);
    }
    out.push('\n');
    out
}

/// The heading before the first step of an iteration (runs with more than one).
pub fn iteration(index: u32, count: u32) -> String {
    format!("Iteration {}/{count}\n", index + 1)
}

/// The lines of one finished request, ending with a newline. `method` is the request's
/// method (the report has none when the request was not built).
pub fn step(step: &RunStep, method: &str) -> String {
    let report = &step.report;
    let name = one_line(&step.request_name);
    let mut out = String::new();
    if step.skipped {
        let _ = writeln!(out, "- {method} {name}  skipped");
        return out;
    }
    let mark = if step.passed { '✓' } else { '✗' };
    match (&report.error, &report.response) {
        (Some(e), _) => {
            let _ = writeln!(
                out,
                "{mark} {method} {name}  {}",
                one_line(&error_message(e))
            );
        }
        (None, Some(r)) => {
            let _ = writeln!(
                out,
                "{mark} {method} {name}  {}  {} ms",
                r.status,
                r.total.as_millis()
            );
        }
        (None, None) => {
            let _ = writeln!(out, "{mark} {method} {name}");
        }
    }
    for t in report.tests.iter().filter(|t| !t.passed) {
        let why = t.error.as_deref().unwrap_or("failed");
        let _ = writeln!(out, "    ✗ {}: {}", one_line(&t.name), indented(why));
    }
    for a in report.assertions.iter().filter(|a| !a.passed) {
        let _ = writeln!(
            out,
            "    ✗ {}: {}",
            assertion_name(a),
            indented(&assertion_message(a))
        );
    }
    for n in report.notes.iter().filter(|n| n.level != NoteLevel::Info) {
        let _ = writeln!(out, "    ! {}: {}", n.source, indented(&n.text));
    }
    for l in &report.logs {
        let _ = writeln!(out, "    > {}", indented(&l.text));
    }
    out
}

/// The summary at the end.
pub fn summary(s: &RunSummary) -> String {
    let mut out = format!(
        "\nRequests: {} ({} passed, {} failed, {} skipped)\nTests: {} passed, {} failed\nAssertions: {} passed, {} failed\nDuration: {}\n",
        s.requests,
        s.passed,
        s.failed,
        s.skipped,
        s.tests_passed,
        s.tests_failed,
        s.assertions_passed,
        s.assertions_failed,
        duration(s.duration_ms),
    );
    if s.stopped {
        out.push_str("The run was stopped early.\n");
    }
    out
}

fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.2} s", ms as f64 / 1000.0)
    }
}

fn one_line(s: &str) -> String {
    clean(&s.replace(['\r', '\n'], " "))
}

/// Continuation lines line up under the first.
fn indented(s: &str) -> String {
    clean(s.trim_end()).replace('\n', "\n      ")
}

/// Control characters from responses and scripts (terminal escapes) are spelled out.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() && c != '\n' && c != '\t' {
            let _ = write!(out, "\\u{:04x}", c as u32);
        } else {
            out.push(c);
        }
    }
    out
}
