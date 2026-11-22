// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The script sandbox in the browser (wave 7): every case file of `crates/rt-script/tests/cases`
//! through the same engine the web app uses (wasmi in WebAssembly), in headless Chrome. The
//! native runner is `crates/rt-script/tests/cases.rs`; both must pass the same files.
//!
//! Run: `RT_CONFORMANCE_PORT=47861 scripts/conformance-wasm.sh -- --test scripts`

#![cfg(target_arch = "wasm32")]

use std::cell::Cell;
use std::rc::Rc;

use rt_core::model::ScriptPhase;
use rt_core::script::{ScriptCall, ScriptInfo, ScriptRequest, ScriptSource};
use rt_core::services::ScriptRunner;
use rt_script::{Limits, Sandbox};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::{console_log, wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn script_case_files_parse() {
    let (cases, problems) = rt_script::cases::all();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(cases.len() >= 20, "{} cases", cases.len());
}

#[wasm_bindgen_test]
async fn script_core_cases_in_the_browser() {
    rt_script::cases::run_all("core/", |line| console_log!("{line}")).await;
}

#[wasm_bindgen_test]
async fn script_shim_and_module_cases_in_the_browser() {
    rt_script::cases::run_all("shims/", |line| console_log!("{line}")).await;
    rt_script::cases::run_all("modules/", |line| console_log!("{line}")).await;
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let set_timeout: js_sys::Function =
            js_sys::Reflect::get(&js_sys::global(), &"setTimeout".into())
                .unwrap()
                .unchecked_into();
        set_timeout
            .call2(&wasm_bindgen::JsValue::NULL, &resolve, &ms.into())
            .unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

/// An endless loop must not freeze the page: timers keep firing while the script runs.
#[wasm_bindgen_test]
async fn an_endless_loop_does_not_block_the_page() {
    let done = Rc::new(Cell::new(false));
    let worst = Rc::new(Cell::new(0.0f64));
    let ticks = Rc::new(Cell::new(0u32));
    {
        let (done, worst, ticks) = (done.clone(), worst.clone(), ticks.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let mut last = js_sys::Date::now();
            while !done.get() {
                sleep(5).await;
                let now = js_sys::Date::now();
                worst.set(worst.get().max(now - last));
                ticks.set(ticks.get() + 1);
                last = now;
            }
        });
    }
    // Let the ticker start.
    sleep(20).await;
    let sandbox = Sandbox::new(Limits {
        time_ms: 1500,
        ..Limits::default()
    });
    let started = js_sys::Date::now();
    let outcome = sandbox
        .run(ScriptCall {
            phase: ScriptPhase::BeforeRequest,
            scripts: vec![ScriptSource {
                origin: "request".into(),
                code: "while (true) {}".into(),
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
        })
        .await;
    let elapsed = js_sys::Date::now() - started;
    done.set(true);
    sleep(20).await;
    console_log!(
        "endless loop: {elapsed:.0} ms, {} timer ticks, worst gap {:.0} ms",
        ticks.get(),
        worst.get()
    );
    assert_eq!(
        outcome.error.as_deref(),
        Some("Script stopped: time limit of 1500 ms exceeded")
    );
    assert!((1400.0..4000.0).contains(&elapsed), "{elapsed} ms");
    assert!(ticks.get() > 20, "{} ticks", ticks.get());
    assert!(worst.get() <= 100.0, "worst gap {} ms", worst.get());
}
