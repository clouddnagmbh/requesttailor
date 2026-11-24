// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19 interface: the additive fields are optional in old JSON and left out when empty;
//! trace injection is off by default and never overwrites a header.

use rt_core::send::SendReport;
use rt_core::trace::{self, TraceConfig};
use rt_core::transport::HttpRequest;
use serde_json::json;

fn old_report() -> serde_json::Value {
    json!({
        "version": 1, "request_path": "a.yml", "environment": null, "request": null,
        "response": null, "variables": [], "unresolved": [], "error": null
    })
}

#[test]
fn a_report_from_before_wave_19_reads_and_writes_back_unchanged() {
    let report: SendReport = serde_json::from_value(old_report()).unwrap();
    assert!(report.trace.is_none());
    assert_eq!(serde_json::to_value(&report).unwrap(), old_report());
}

#[test]
fn trace_injection_is_off_by_default_and_leaves_the_request_alone() {
    let mut request = HttpRequest {
        method: "GET".into(),
        url: "https://example.com/".into(),
        headers: Vec::new(),
        body: None,
        options: Default::default(),
    };
    let before = request.clone();
    assert!(trace::inject(&mut request, &TraceConfig::default()).is_none());
    assert_eq!(request, before);
}
