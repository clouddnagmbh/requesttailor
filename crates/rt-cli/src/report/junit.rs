// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! JUnit XML in the shape GitHub, GitLab and Jenkins read:
//!
//! - `<testsuites>` for the run (name: the collection), totals and wall time;
//! - one `<testsuite>` per request and iteration, named by the request path, plus
//!   ` [iteration n]` when the run has more than one iteration;
//! - one `<testcase>` per test (`test(...)` in scripts) and per declarative assertion; a
//!   failed one has a `<failure message=…>`;
//! - a testcase named `request` when the request itself has something to say: an `<error>`
//!   when it failed (transport, preparation, auth, plugin, pre-request script), `<skipped/>`
//!   when a script skipped it, or a passing case when it has no tests and no assertions (so
//!   every request shows up);
//! - script output and warnings in `<system-out>`.
//!
//! Times are seconds. Text is escaped for XML 1.0; characters XML cannot carry at all (most
//! control characters, U+FFFE, U+FFFF) are written as `\uXXXX`.

use std::fmt::Write as _;

use rt_core::runner::{RunStep, RunSummary};
use rt_core::services::NoteLevel;

use super::{RunInfo, assertion_message, assertion_name, error_kind, error_message};

/// The whole document, with a final newline.
pub fn junit(info: &RunInfo, summary: &RunSummary, steps: &[RunStep]) -> String {
    let suites: Vec<Suite> = steps.iter().map(|s| suite(info, s)).collect();
    let total = |f: fn(&Suite) -> usize| suites.iter().map(f).sum::<usize>();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{}\">",
        attr(&info.collection),
        total(|s| s.cases.len()),
        total(Suite::failures),
        total(Suite::errors),
        total(Suite::skipped),
        seconds(summary.duration_ms as f64 / 1000.0),
    );
    for s in &suites {
        s.write(&mut out);
    }
    out.push_str("</testsuites>\n");
    out
}

enum Outcome {
    Passed,
    Failure { message: String, kind: &'static str },
    Error { message: String, kind: &'static str },
    Skipped,
}

struct Case {
    name: String,
    time: f64,
    outcome: Outcome,
}

struct Suite {
    name: String,
    time: f64,
    cases: Vec<Case>,
    output: String,
}

impl Suite {
    fn failures(&self) -> usize {
        self.count(|o| matches!(o, Outcome::Failure { .. }))
    }

    fn errors(&self) -> usize {
        self.count(|o| matches!(o, Outcome::Error { .. }))
    }

    fn skipped(&self) -> usize {
        self.count(|o| matches!(o, Outcome::Skipped))
    }

    fn count(&self, f: impl Fn(&Outcome) -> bool) -> usize {
        self.cases.iter().filter(|c| f(&c.outcome)).count()
    }

    fn write(&self, out: &mut String) {
        let name = attr(&self.name);
        let _ = writeln!(
            out,
            "  <testsuite name=\"{name}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{}\">",
            self.cases.len(),
            self.failures(),
            self.errors(),
            self.skipped(),
            seconds(self.time),
        );
        for c in &self.cases {
            let head = format!(
                "    <testcase name=\"{}\" classname=\"{name}\" time=\"{}\"",
                attr(&c.name),
                seconds(c.time)
            );
            match &c.outcome {
                Outcome::Passed => {
                    let _ = writeln!(out, "{head}/>");
                }
                Outcome::Skipped => {
                    let _ = writeln!(out, "{head}>\n      <skipped/>\n    </testcase>");
                }
                Outcome::Failure { message, kind } | Outcome::Error { message, kind } => {
                    let tag = if matches!(c.outcome, Outcome::Error { .. }) {
                        "error"
                    } else {
                        "failure"
                    };
                    let _ = writeln!(
                        out,
                        "{head}>\n      <{tag} message=\"{}\" type=\"{kind}\">{}</{tag}>\n    </testcase>",
                        attr(message),
                        text(message),
                    );
                }
            }
        }
        if !self.output.is_empty() {
            let _ = writeln!(out, "    <system-out>{}</system-out>", text(&self.output));
        }
        out.push_str("  </testsuite>\n");
    }
}

fn suite(info: &RunInfo, step: &RunStep) -> Suite {
    let report = &step.report;
    let name = if info.iterations > 1 {
        format!("{} [iteration {}]", step.request_path, step.iteration + 1)
    } else {
        step.request_path.clone()
    };
    let time = report
        .response
        .as_ref()
        .map_or(0.0, |r| r.total.as_secs_f64());
    let mut cases = Vec::new();
    if step.skipped {
        cases.push(Case {
            name: "request".into(),
            time: 0.0,
            outcome: Outcome::Skipped,
        });
    } else if let Some(e) = &report.error {
        cases.push(Case {
            name: "request".into(),
            time,
            outcome: Outcome::Error {
                message: error_message(e),
                kind: error_kind(e),
            },
        });
    }
    for t in &report.tests {
        cases.push(Case {
            name: t.name.clone(),
            time: 0.0,
            outcome: if t.passed {
                Outcome::Passed
            } else {
                Outcome::Failure {
                    message: t.error.clone().unwrap_or_else(|| "test failed".into()),
                    kind: "test",
                }
            },
        });
    }
    for a in &report.assertions {
        cases.push(Case {
            name: assertion_name(a),
            time: 0.0,
            outcome: if a.passed {
                Outcome::Passed
            } else {
                Outcome::Failure {
                    message: assertion_message(a),
                    kind: "assertion",
                }
            },
        });
    }
    if cases.is_empty() {
        cases.push(Case {
            name: "request".into(),
            time,
            outcome: Outcome::Passed,
        });
    }
    let mut output = String::new();
    for log in &report.logs {
        let level = serde_json::to_value(log.level)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let _ = writeln!(output, "[{level}] {}: {}", log.origin, log.text);
    }
    for note in &report.notes {
        let level = match note.level {
            NoteLevel::Info => "info",
            NoteLevel::Warning => "warning",
            NoteLevel::Error => "error",
        };
        let _ = writeln!(output, "[{level}] {}: {}", note.source, note.text);
    }
    Suite {
        name,
        time,
        cases,
        output,
    }
}

fn seconds(s: f64) -> String {
    format!("{s:.3}")
}

/// Characters XML 1.0 allows.
fn allowed(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn escape(s: &str, in_attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if in_attr => out.push_str("&quot;"),
            '\'' if in_attr => out.push_str("&apos;"),
            // Literal whitespace in attributes is normalized to spaces by parsers; a literal
            // CR anywhere becomes LF.
            '\n' if in_attr => out.push_str("&#10;"),
            '\t' if in_attr => out.push_str("&#9;"),
            '\r' => out.push_str("&#13;"),
            c if !allowed(c) => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn attr(s: &str) -> String {
    escape(s, true)
}

fn text(s: &str) -> String {
    escape(s, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(
            attr("a<b>&\"'\n\t\r"),
            "a&lt;b&gt;&amp;&quot;&apos;&#10;&#9;&#13;"
        );
        assert_eq!(text("a\"'\n\t"), "a\"'\n\t");
        assert_eq!(
            text("\u{1b}[31m\u{0}\u{ffff}é😀"),
            "\\u001b[31m\\u0000\\uffffé😀"
        );
        assert_eq!(text("]]>"), "]]&gt;");
    }

    #[test]
    fn seconds_have_three_decimals() {
        assert_eq!(seconds(0.0), "0.000");
        assert_eq!(seconds(1.2344), "1.234");
        assert_eq!(seconds(12.0), "12.000");
    }
}
