// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What the case files cannot express: behaviour across runs.

use rt_core::model::ScriptPhase;
use rt_core::script::{ScriptCall, ScriptInfo, ScriptOutcome, ScriptRequest, ScriptSource};
use rt_core::services::ScriptRunner;
use rt_script::{Limits, Sandbox};

fn run(sandbox: &Sandbox, code: &str) -> ScriptOutcome {
    rt_script::block_on(sandbox.run(ScriptCall {
        phase: ScriptPhase::BeforeRequest,
        scripts: vec![ScriptSource {
            origin: "request".into(),
            code: code.into(),
        }],
        request: ScriptRequest {
            method: "GET".into(),
            url: "http://h/".into(),
            headers: Vec::new(),
            body: None,
        },
        response: None,
        variables: Vec::new(),
        info: ScriptInfo::default(),
        sender: None,
    }))
}

fn logs(o: &ScriptOutcome) -> Vec<String> {
    o.logs.iter().map(|l| l.text.clone()).collect()
}

#[test]
fn every_run_starts_fresh() {
    let sandbox = Sandbox::default();
    let first = run(
        &sandbox,
        "globalThis.leak = 1; var v = 2; Array.prototype.evil = 3; console.log(typeof leak);",
    );
    assert_eq!(logs(&first), ["number"]);
    let second = run(
        &sandbox,
        "console.log(typeof leak, typeof v, typeof [].evil);",
    );
    assert_eq!(logs(&second), ["undefined undefined undefined"]);
}

#[test]
fn math_random_differs_between_runs() {
    let sandbox = Sandbox::default();
    let a = run(&sandbox, "console.log(Math.random(), Math.random());");
    let b = run(&sandbox, "console.log(Math.random(), Math.random());");
    assert_ne!(logs(&a), logs(&b));
    let values: Vec<f64> = logs(&a)[0].split(' ').map(|v| v.parse().unwrap()).collect();
    assert!(values.iter().all(|v| (0.0..1.0).contains(v)), "{values:?}");
    assert_ne!(values[0], values[1]);
}

#[test]
fn limits_are_per_sandbox() {
    let quick = Sandbox::new(Limits {
        time_ms: 200,
        ..Limits::default()
    });
    let started = web_time::Instant::now();
    let o = run(&quick, "while (true) {}");
    assert_eq!(
        o.error.as_deref(),
        Some("Script stopped: time limit of 200 ms exceeded")
    );
    assert!(started.elapsed() < web_time::Duration::from_secs(2));
    // The engine is still fine afterwards.
    let o = run(&quick, "console.log('ok')");
    assert_eq!(o.error, None);
    assert_eq!(logs(&o), ["ok"]);
}

#[test]
fn many_runs_in_a_row() {
    let sandbox = Sandbox::default();
    for i in 0..20 {
        let o = run(&sandbox, &format!("rt.vars.set('i', {i});"));
        assert_eq!(o.error, None);
        assert_eq!(
            o.var_changes[0].value.as_deref(),
            Some(i.to_string().as_str())
        );
    }
}
