// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --trace` (wave 19, strand A): prints the trace id of the send on stderr (with
//! `--json` it is also in the report's `trace`). The `traceparent` header itself is set by
//! the collection's `extensions.requesttailor.trace.inject`, not by this flag: the flag
//! never changes what goes on the wire.

use rt_core::send::SendReport;

pub const HELP: &str = "      --trace               Print the trace id of the send on stderr\n";

/// The line `--trace` prints: the trace id and where it came from, or why there is none.
pub fn line(report: &SendReport) -> String {
    match &report.trace {
        Some(t) => {
            let origin = if t.injected {
                "sent by rt as traceparent"
            } else if report.request.as_ref().is_some_and(|r| {
                r.headers
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case("traceparent"))
            }) {
                "from the request's own traceparent"
            } else {
                "from the response"
            };
            if t.span_id.is_empty() {
                format!("trace {} ({origin})", t.trace_id)
            } else {
                format!("trace {} span {} ({origin})", t.trace_id, t.span_id)
            }
        }
        None => "no trace id (tracing is off for this collection: extensions.requesttailor.trace.inject)".into(),
    }
}
