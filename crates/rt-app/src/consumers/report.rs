// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The two readable forms of a [`ConsumerReport`]: plain text and GitHub Actions workflow
//! commands (`rt verify-consumers --out text|github`). The JSON form is the serialised report.

use super::{ConsumerReport, ConsumerResult};

fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

fn head(c: &ConsumerResult) -> String {
    let mut line = format!(
        "{} request{}",
        c.requests,
        if c.requests == 1 { "" } else { "s" }
    );
    if c.skipped > 0 {
        line.push_str(&format!(", {} left out", c.skipped));
    }
    if let Some(commit) = &c.commit {
        line.push_str(&format!(", commit {}", short(commit)));
    }
    line
}

/// One block per consumer; a broken one names the request, the assertion and the difference.
pub fn text(report: &ConsumerReport) -> String {
    let mut out = String::new();
    for c in &report.consumers {
        if let Some(error) = &c.error {
            out.push_str(&format!(
                "Consumer {} could not be verified ({}): {error}\n",
                c.name,
                head(c)
            ));
        } else if c.failures.is_empty() {
            out.push_str(&format!("Consumer {} is fine ({})\n", c.name, head(c)));
        } else {
            out.push_str(&format!("Consumer {} breaks ({}):\n", c.name, head(c)));
        }
        for f in &c.failures {
            out.push_str(&format!(
                "  {}: {} ({})\n    {}\n",
                f.request, f.assertion, f.request_path, f.difference
            ));
        }
    }
    let broken = report
        .consumers
        .iter()
        .filter(|c| !c.failures.is_empty() || c.error.is_some())
        .count();
    out.push_str(&format!(
        "{} of {} consumer{} broken\n",
        broken,
        report.consumers.len(),
        if report.consumers.len() == 1 { "" } else { "s" }
    ));
    out
}

fn property(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// GitHub Actions annotations: one `::error` per failure and per consumer that could not run.
pub fn github(report: &ConsumerReport) -> String {
    let mut out = String::new();
    for c in &report.consumers {
        if let Some(error) = &c.error {
            out.push_str(&format!(
                "::error title={}::{}\n",
                property(&format!("Consumer {} could not be verified", c.name)),
                data(error)
            ));
        }
        for f in &c.failures {
            out.push_str(&format!(
                "::error title={}::{}\n",
                property(&format!("Consumer {} breaks: {}", c.name, f.request)),
                data(&format!("{}: {}", f.assertion, f.difference))
            ));
        }
    }
    out
}
