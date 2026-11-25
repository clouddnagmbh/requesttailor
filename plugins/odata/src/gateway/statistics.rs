// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The `sap-statistics` response header as time segments.
//!
//! **Not verified against a real system.** Assumed: a list of `key=number` pairs separated by
//! `,` or `;`, the number in milliseconds unless it carries a unit (`us`, `ms`, `s`), the keys
//! one of the names in [`kind_of`]. The reader is tolerant: what it does not understand is
//! reported back in [`Statistics::unknown`] (the caller makes a note of it), never an error.
//! Sources and assumptions: `testdata/SOURCES.md`.

use rt_core::TimingSegment;

/// The segments of one header and the parts that were not understood.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Statistics {
    /// Gateway, application, database, then the rest of the total (only when positive).
    pub segments: Vec<TimingSegment>,
    pub unknown: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Total,
    Gateway,
    Application,
    Database,
}

fn kind_of(key: &str) -> Option<Kind> {
    Some(match key {
        "total" | "tot" | "sum" => Kind::Total,
        "gw" | "fw" | "gateway" | "hub" | "framework" => Kind::Gateway,
        "app" | "application" | "be" | "backend" => Kind::Application,
        "db" | "database" => Kind::Database,
        _ => return None,
    })
}

/// A number with an optional unit, in milliseconds.
fn millis(value: &str) -> Option<f64> {
    let value = value.trim().to_ascii_lowercase();
    let (number, factor) = if let Some(n) = value.strip_suffix("us").or(value.strip_suffix("µs")) {
        (n, 0.001)
    } else if let Some(n) = value.strip_suffix("ms") {
        (n, 1.0)
    } else if let Some(n) = value.strip_suffix('s') {
        (n, 1000.0)
    } else {
        (value.as_str(), 1.0)
    };
    let ms = number.trim().parse::<f64>().ok()? * factor;
    (ms.is_finite() && ms >= 0.0).then_some(ms)
}

fn segment(id: &str, label: &str, ms: f64) -> TimingSegment {
    TimingSegment {
        id: id.into(),
        label: label.into(),
        ms: ms.round() as u64,
        parent: Some(rt_core::segments::SEGMENT_PARENT_SERVER.into()),
        offset_ms: None,
    }
}

/// Reads the header value.
pub fn parse(header: &str) -> Statistics {
    let mut total: Option<f64> = None;
    let mut parts: [Option<f64>; 3] = [None; 3];
    let mut unknown = Vec::new();
    for token in header.split([',', ';']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let known = token.split_once('=').and_then(|(key, value)| {
            let kind = kind_of(key.trim().to_ascii_lowercase().as_str())?;
            Some((kind, millis(value)?))
        });
        match known {
            Some((Kind::Total, ms)) => *total.get_or_insert(0.0) += ms,
            Some((Kind::Gateway, ms)) => *parts[0].get_or_insert(0.0) += ms,
            Some((Kind::Application, ms)) => *parts[1].get_or_insert(0.0) += ms,
            Some((Kind::Database, ms)) => *parts[2].get_or_insert(0.0) += ms,
            None => unknown.push(token.to_owned()),
        }
    }
    let mut segments = Vec::new();
    for (ms, id, label) in [
        (parts[0], "gateway", "Gateway"),
        (parts[1], "application", "Application"),
        (parts[2], "database", "Database"),
    ] {
        if let Some(ms) = ms {
            segments.push(segment(id, label, ms));
        }
    }
    if let Some(total) = total
        && !segments.is_empty()
    {
        let summed: f64 = parts.iter().flatten().sum();
        if total - summed >= 0.5 {
            segments.push(segment("rest", "Other", total - summed));
        }
    }
    Statistics { segments, unknown }
}
