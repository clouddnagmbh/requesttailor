// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18 interface: the additive fields are optional in old JSON, left out when empty, and
//! the hooks the strands fill in answer "nothing" until then.

use rt_core::inbox::{CapturedRequest, InboxEvent};
use rt_core::paging::{CursorParam, CursorPlace, PagingRule, PagingSpec};
use rt_core::ratelimit::{Throttle, ThrottleMode};
use rt_core::runner::{RunEvent, RunPlan};
use rt_core::send::SendReport;
use serde_json::json;

fn old_report() -> serde_json::Value {
    json!({
        "version": 1, "request_path": "a.yml", "environment": null, "request": null,
        "response": null, "variables": [], "unresolved": [], "error": null
    })
}

#[test]
fn a_report_from_before_wave_18_reads_and_writes_back_unchanged() {
    let report: SendReport = serde_json::from_value(old_report()).unwrap();
    assert!(report.segments.is_empty() && report.checks.is_empty() && report.rate_limit.is_none());
    assert_eq!(serde_json::to_value(&report).unwrap(), old_report());
}

#[test]
fn a_plan_without_throttle_reads_and_omits_the_default() {
    let plan: RunPlan = serde_json::from_value(json!({
        "requests": [], "data": [], "iterations": 1, "delay_ms": 0, "bail": false
    }))
    .unwrap();
    assert_eq!(plan.throttle, Throttle::default());
    assert!(
        serde_json::to_value(&plan)
            .unwrap()
            .get("throttle")
            .is_none()
    );
    let mut respecting = plan;
    respecting.throttle.mode = ThrottleMode::Respect;
    assert_eq!(
        serde_json::to_value(&respecting).unwrap()["throttle"]["mode"],
        "respect"
    );
}

#[test]
fn the_waiting_event_is_a_run_event() {
    let event: RunEvent = serde_json::from_value(
        json!({ "kind": "waiting", "request_path": "a.yml", "until": 5, "reason": "limit" }),
    )
    .unwrap();
    assert!(matches!(event, RunEvent::Waiting { until: 5, .. }));
}

#[test]
fn paging_rules_round_trip() {
    let spec = PagingSpec {
        rule: PagingRule::Cursor {
            next: "$.next".into(),
            param: CursorParam {
                place: CursorPlace::Query,
                name: "cursor".into(),
            },
        },
        merge: Some("$.items".into()),
    };
    let json = serde_json::to_value(&spec).unwrap();
    assert_eq!(json["rule"]["kind"], "cursor");
    assert_eq!(serde_json::from_value::<PagingSpec>(json).unwrap(), spec);
}

#[test]
fn inbox_events_are_tagged() {
    let request = CapturedRequest {
        id: 1,
        at: 2,
        method: "POST".into(),
        target: "/".into(),
        headers: Vec::new(),
        body: None,
        size: 0,
    };
    let json = serde_json::to_value(InboxEvent::Request {
        request: Box::new(request),
    })
    .unwrap();
    assert_eq!(json["kind"], "request");
    assert_eq!(json["request"]["id"], 1);
}

#[test]
fn the_hooks_of_the_strands_answer_nothing_until_they_are_built() {
    // Strand D built the rate-limit parser (tests/ratelimit.rs); only the others still wait.
    assert!(rt_core::jwt::parse("a.b.c").is_err());
}
