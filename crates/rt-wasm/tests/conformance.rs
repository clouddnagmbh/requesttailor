// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Browser conformance runner: every case in `testdata/conformance/cases.json` through the
//! WASM core (`rt_wasm::send_files`, `FetchTransport`) in headless Chrome, against
//! `rt-testserver` on its default port. Same harness as the native runner.
//!
//! Run: `scripts/conformance-wasm.sh` (add `-- --include-ignored` before strands A-C are merged).

#![cfg(target_arch = "wasm32")]

#[path = "../../../testdata/conformance/harness.rs"]
mod harness;

include!(concat!(env!("OUT_DIR"), "/testdata_files.rs"));

use harness::Target;
use rt_core::transport::fetch::FetchTransport;
use rt_format::FileSet;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::{console_log, wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

/// The files of one collection, keys relative to its root.
fn file_set(collection_path: &str) -> FileSet {
    let prefix = format!("{collection_path}/");
    let mut set = FileSet::default();
    for (key, bytes) in TESTDATA {
        if let Some(rel) = key.strip_prefix(&prefix) {
            set.files.insert(rel.to_owned(), bytes.to_vec());
        }
    }
    set
}

async fn fetch_text(url: &str) -> Result<(u16, String), String> {
    let window = web_sys::window().ok_or("no window")?;
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("fetch {url}: {e:?}"))?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| "not a Response")?;
    let text = JsFuture::from(response.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok((response.status(), text.as_string().unwrap_or_default()))
}

#[wasm_bindgen_test]
fn case_files_are_embedded() {
    let suite = harness::suite();
    let problems = harness::validate(&suite, |p| TESTDATA.iter().any(|(k, _)| *k == p));
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The harness itself, independent of the core: the page reaches the test server (CORS
/// included) through plain `fetch`. If this fails, the server is not running on
/// `harness::BROWSER_PORT` or the browser setup is broken.
#[wasm_bindgen_test]
async fn browser_reaches_the_test_server() {
    let url = format!("{}/echo/harness?x=1", harness::base(harness::BROWSER_PORT));
    let (status, text) = fetch_text(&url).await.unwrap_or_else(|e| {
        panic!(
            "{e}: is rt-testserver running on port {}?",
            harness::BROWSER_PORT
        )
    });
    assert_eq!(status, 200, "{text}");
    let echo: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(echo["method"], "GET");
    assert_eq!(echo["path"], "/echo/harness");
    assert_eq!(echo["query"], "x=1");
}

#[wasm_bindgen_test]
#[ignore = "needs strands A-C (fetch transport, format, prepare); run after merge"]
async fn conformance_browser() {
    let port = harness::BROWSER_PORT;
    let suite = harness::suite();
    let mut outcome = harness::Outcome::new();
    for case in &suite.cases {
        if let Some(reason) = &case.browser {
            console_log!("{}", outcome.skip(case, reason));
            continue;
        }
        let plan = suite.plan(case, port);
        let options = rt_wasm::Options {
            environment: plan.environment.clone(),
            vars: plan.vars.iter().cloned().collect(),
            globals: Default::default(),
            body_limit: plan.body_limit,
        };
        let files = file_set(&plan.collection_path);
        let line = match rt_wasm::send_files(
            &files,
            &plan.request,
            &options,
            &FetchTransport::new(),
        )
        .await
        {
            Err(e) => outcome.record(case, vec![format!("cannot load: {e}")], ""),
            Ok(report) => {
                let json = serde_json::to_value(&report).unwrap();
                let failures = harness::check(case, Target::Browser, port, &json);
                outcome.record(case, failures, &json.to_string())
            }
        };
        console_log!("{line}");
    }
    outcome.finish();
}

/// The JS boundary: files as a JS object, options as a JS object, report as a JS object
/// that matches the CLI's JSON.
#[wasm_bindgen_test]
#[ignore = "needs strands A-C (fetch transport, format, prepare); run after merge"]
async fn js_api_sends_an_unchanged_bruno_collection() {
    let port = harness::BROWSER_PORT;
    let suite = harness::suite();
    let case = suite
        .cases
        .iter()
        .find(|c| c.collection == "bruno-methods" && c.request == "GET-Request.yml")
        .unwrap();
    let plan = suite.plan(case, port);
    let files = js_sys::Object::new();
    for (path, bytes) in file_set(&plan.collection_path).files {
        let value: wasm_bindgen::JsValue = match String::from_utf8(bytes) {
            Ok(s) => s.into(),
            Err(e) => js_sys::Uint8Array::from(e.as_bytes()).into(),
        };
        js_sys::Reflect::set(&files, &path.into(), &value).unwrap();
    }
    let vars: serde_json::Map<String, serde_json::Value> =
        plan.vars.into_iter().map(|(k, v)| (k, v.into())).collect();
    let options = js_sys::JSON::parse(&serde_json::json!({ "vars": vars }).to_string()).unwrap();
    let report = rt_wasm::send_js(files.into(), plan.request.clone(), options)
        .await
        .unwrap_or_else(|e| panic!("rejected: {e:?}"));
    let json: serde_json::Value =
        serde_json::from_str(&String::from(js_sys::JSON::stringify(&report).unwrap())).unwrap();
    let failures = harness::check(case, Target::Browser, port, &json);
    assert!(failures.is_empty(), "{failures:#?}\n{json:#}");
}

#[wasm_bindgen_test]
async fn js_api_rejects_bad_input() {
    let err = rt_wasm::send_js(42.into(), "a.yml".into(), wasm_bindgen::JsValue::UNDEFINED)
        .await
        .unwrap_err();
    assert!(err.is_instance_of::<js_sys::TypeError>(), "{err:?}");
    let options = js_sys::JSON::parse(r#"{"bogus": 1}"#).unwrap();
    let err = rt_wasm::send_js(js_sys::Object::new().into(), "a.yml".into(), options)
        .await
        .unwrap_err();
    assert!(err.is_instance_of::<js_sys::TypeError>(), "{err:?}");
}
