// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What a script costs. The budget (wave 7): a send with one trivial script adds at most
//! about 20 ms in a native release build. Measured here without the network:
//!
//! `cargo test -p rt-script --release --test perf -- --ignored --nocapture`
//!
//! The non-ignored test checks a generous bound in any build so a regression shows up.

use rt_core::model::ScriptPhase;
use rt_core::script::{ScriptCall, ScriptInfo, ScriptRequest, ScriptSource};
use rt_core::services::ScriptRunner;
use rt_script::Sandbox;
use web_time::{Duration, Instant};

fn trivial() -> ScriptCall<'static> {
    ScriptCall {
        phase: ScriptPhase::BeforeRequest,
        scripts: vec![ScriptSource {
            origin: "request".into(),
            code: "rt.vars.set('ts', String(Date.now())); rt.request.headers.set('X-Trace', 'a');"
                .into(),
        }],
        request: ScriptRequest {
            method: "GET".into(),
            url: "http://h/x".into(),
            headers: vec![("Accept".into(), "application/json".into())],
            body: None,
        },
        response: None,
        variables: Vec::new(),
        info: ScriptInfo::default(),
        sender: None,
    }
}

fn measure(runs: u32) -> (Duration, Duration) {
    let sandbox = Sandbox::default();
    let start = Instant::now();
    let first = rt_script::block_on(sandbox.run(trivial()));
    assert_eq!(first.error, None);
    let cold = start.elapsed();
    let start = Instant::now();
    for _ in 0..runs {
        let o = rt_script::block_on(sandbox.run(trivial()));
        assert_eq!(o.error, None);
        assert!(o.request.is_some());
    }
    (cold, start.elapsed() / runs)
}

#[test]
fn a_trivial_script_is_cheap() {
    let (_, per_run) = measure(5);
    // Debug builds are slower; the release budget is checked by the ignored test below.
    assert!(per_run < Duration::from_millis(500), "{per_run:?} per run");
}

#[test]
#[ignore = "measurement; run with --release"]
fn report_trivial_script_cost() {
    let (cold, per_run) = measure(50);
    println!("engine start (compile + init, once per process): {cold:?}");
    println!("one trivial before-request script: {per_run:?}");
    if !cfg!(debug_assertions) {
        assert!(per_run < Duration::from_millis(20), "{per_run:?} per run");
    }
}
