// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Native conformance runner: every case in `testdata/conformance/cases.json` through
//! `rt_core::send::send` with `NativeTransport`, against an in-process `rt-testserver`.
//! The browser runner is `crates/rt-wasm/tests/conformance.rs`; both use the same harness.
//!
//! Run: `cargo test -p rt-cli --test conformance -- --include-ignored --nocapture`

#[path = "../../../testdata/conformance/harness.rs"]
mod harness;

use std::path::{Path, PathBuf};
use std::time::Duration;

use harness::{Plan, Target};
use rt_cli::{DirFiles, SendArgs};
use rt_core::send::{SendOptions, SendReport};
use rt_core::transport::native::NativeTransport;

/// Generous: the slowest case waits for a 300 ms timeout.
const CASE_TIMEOUT: Duration = Duration::from_secs(20);

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

#[test]
fn case_file_is_valid() {
    let suite = harness::suite();
    assert!(suite.cases.len() >= 30, "{} cases", suite.cases.len());
    let problems = harness::validate(&suite, |p| testdata().join(p).is_file());
    assert!(problems.is_empty(), "{problems:#?}");
    assert_eq!(harness::DEFAULT_BROWSER_PORT, rt_testserver::DEFAULT_PORT);
}

/// The comparison itself, without any strand: a hand-made report passes, a wrong one fails.
#[test]
fn harness_detects_mismatches() {
    let suite = harness::suite();
    let case = suite
        .cases
        .iter()
        .find(|c| c.request == "basics/status.yml")
        .unwrap();
    let mut report = serde_json::json!({
        "version": 1,
        "request_path": "basics/status.yml",
        "environment": "local",
        "request": {"method": "GET", "url": "http://127.0.0.1:1/status/404", "headers": [], "body_size": 0},
        "response": {
            "status": 404, "version": "HTTP/1.1", "headers": [], "url": "http://127.0.0.1:1/status/404",
            "redirects": [], "body": {"encoding": "utf8", "data": "{\"status\":404}"},
            "size": 14, "truncated": false,
            "timing": {"dns": null, "connect": null, "tls": null, "ttfb": 1.0}, "total": 2.0
        },
        "variables": [], "unresolved": [], "error": null
    });
    assert_eq!(
        harness::check(case, Target::Native, 1, &report),
        Vec::<String>::new()
    );
    report["response"]["status"] = 200.into();
    report["error"] = serde_json::json!({"kind": "transport", "message": {"kind": "timeout"}});
    let failures = harness::check(case, Target::Native, 1, &report);
    assert_eq!(failures.len(), 2, "{failures:#?}");

    // Browser normalization: connect and blocked are the same network failure.
    let refused = suite
        .cases
        .iter()
        .find(|c| c.request == "transport/refused.yml")
        .unwrap();
    let blocked = serde_json::json!({
        "version": 1, "request_path": "transport/refused.yml", "request": {}, "response": null,
        "variables": [], "unresolved": [],
        "error": {"kind": "transport", "message": {"kind": "blocked", "message": "Failed to fetch"}}
    });
    assert!(harness::check(refused, Target::Browser, 1, &blocked).is_empty());
    assert!(!harness::check(refused, Target::Native, 1, &blocked).is_empty());
}

async fn run_case(plan: Plan) -> Result<SendReport, String> {
    let root = testdata().join(&plan.collection_path);
    let doc = rt_cli::load_collection(&root)?;
    let files = DirFiles::new(&root).map_err(|e| e.to_string())?;
    let transport = NativeTransport::new().map_err(|e| e.to_string())?;
    let args = SendArgs {
        file: root.join(&plan.request),
        environment: plan.environment,
        vars: plan.vars,
        body_limit: plan.body_limit.unwrap_or(SendOptions::default().body_limit),
        // Same reports on every machine, whatever plugins the developer has installed.
        no_plugins: true,
        ..SendArgs::default()
    };
    Ok(rt_cli::send_request(&doc, &plan.request, &args, &files, &transport).await)
}

fn panic_message(e: tokio::task::JoinError) -> String {
    match e.try_into_panic() {
        Ok(p) => p
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic".into()),
        Err(e) => e.to_string(),
    }
}

#[test]
fn conformance_native() {
    // Same runtime flavour as `rt`.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        let port = rt_testserver::spawn_local().await.port();
        let suite = harness::suite();
        let mut outcome = harness::Outcome::new();
        for case in &suite.cases {
            // Each case in its own task: a panic or hang fails that case, not the run.
            let mut task = tokio::task::spawn_local(run_case(suite.plan(case, port)));
            let line = match tokio::time::timeout(CASE_TIMEOUT, &mut task).await {
                Err(_) => {
                    task.abort();
                    outcome.record(case, vec![format!("no result after {CASE_TIMEOUT:?}")], "")
                }
                Ok(Err(e)) => {
                    outcome.record(case, vec![format!("panicked: {}", panic_message(e))], "")
                }
                Ok(Ok(Err(e))) => outcome.record(case, vec![format!("cannot load: {e}")], ""),
                Ok(Ok(Ok(report))) => {
                    let json = serde_json::to_value(&report).unwrap();
                    let failures = harness::check(case, Target::Native, port, &json);
                    outcome.record(case, failures, &json.to_string())
                }
            };
            println!("{line}");
        }
        outcome.finish();
    });
}
