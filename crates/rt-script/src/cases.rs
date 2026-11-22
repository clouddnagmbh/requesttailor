// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Data-driven sandbox tests (feature `test-cases`): one JSON file per case under
//! `tests/cases/` (`core/` strand A, `shims/` and `modules/` strand B), embedded at build
//! time. The same runner executes them natively (`tests/cases.rs`) and in headless Chrome
//! (`crates/rt-wasm/tests/scripts.rs`). The format mirrors
//! `rt_core::script::{ScriptCall, ScriptOutcome}`: `name`, `phase`, `scripts`, `request`
//! (required), `response` (`body` as UTF-8 text or `body_base64`), `variables`, `info`,
//! `send` (canned answers for `rt.sendRequest`, matched by URL and optional `method`; with
//! `delay_ms` or `error`), `limits` (`time_ms`, `memory_bytes`), `skip` (a reason; the case
//! is parsed but not run) and `expect`. Every key of `expect` is optional and only given keys
//! are compared: `request` (`null`: unchanged), `var_changes`, `tests` (name and passed;
//! `error` as a substring, `origin`), `logs` (level and text), `flow`, `error` (`null` or a
//! substring), `sent` (what `rt.sendRequest` sent, in order), `max_ms` / `min_ms` (wall
//! clock of the run).

use std::collections::BTreeMap;

use rt_core::model::ScriptPhase;
use rt_core::script::{
    Iteration, ScriptCall, ScriptInfo, ScriptOutcome, ScriptRequest, ScriptResponse, ScriptSender,
    ScriptSource, ScriptVar,
};
use rt_core::services::{LocalBoxFuture, ScriptRunner};
use rt_core::vars::VarSource;
use serde::Deserialize;
use serde_json::Value;

use crate::{Limits, Sandbox};

mod files {
    include!(concat!(env!("OUT_DIR"), "/case_files.rs"));
}

/// Every case file: (path below `tests/cases`, JSON).
pub fn files() -> &'static [(&'static str, &'static str)] {
    files::CASE_FILES
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub phase: ScriptPhase,
    pub scripts: Vec<ScriptSource>,
    pub request: ScriptRequest,
    #[serde(default)]
    pub response: Option<CaseResponse>,
    #[serde(default)]
    pub variables: Vec<CaseVar>,
    #[serde(default)]
    pub info: Option<CaseInfo>,
    #[serde(default)]
    pub send: Vec<CannedSend>,
    #[serde(default)]
    pub limits: Option<CaseLimits>,
    /// Why the case is skipped for now (e.g. it needs another strand); it is still parsed.
    #[serde(default)]
    pub skip: Option<String>,
    #[serde(default)]
    pub expect: CaseExpect,
    #[serde(default, rename = "$comment")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub body_base64: Option<String>,
    #[serde(default)]
    pub time_ms: u64,
    #[serde(default)]
    pub url: String,
}

impl CaseResponse {
    fn to_response(&self) -> Result<ScriptResponse, String> {
        let body = match (&self.body, &self.body_base64) {
            (Some(text), None) => text.clone().into_bytes(),
            (None, Some(b64)) => base64(b64)?,
            (None, None) => Vec::new(),
            (Some(_), Some(_)) => return Err("both body and body_base64".into()),
        };
        Ok(ScriptResponse {
            status: self.status,
            headers: self.headers.clone(),
            body,
            time_ms: self.time_ms,
            url: self.url.clone(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseVar {
    pub name: String,
    pub value: String,
    pub source: VarSource,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseInfo {
    #[serde(default)]
    pub request_path: String,
    #[serde(default)]
    pub request_name: String,
    #[serde(default)]
    pub collection_name: String,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub iteration: Option<Iteration>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CannedSend {
    pub url: String,
    /// Only answer a request with this method.
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub response: Option<CaseResponse>,
    /// Fail the send with this message instead.
    #[serde(default)]
    pub error: Option<String>,
    /// Answer after this many milliseconds.
    #[serde(default)]
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseLimits {
    #[serde(default)]
    pub time_ms: Option<u64>,
    #[serde(default)]
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseExpect {
    /// `null`: unchanged.
    #[serde(default, deserialize_with = "some")]
    pub request: Option<Option<ScriptRequest>>,
    #[serde(default)]
    pub var_changes: Option<Vec<Value>>,
    #[serde(default)]
    pub tests: Option<Vec<ExpectTest>>,
    #[serde(default)]
    pub logs: Option<Vec<ExpectLog>>,
    #[serde(default)]
    pub flow: Option<Value>,
    /// `null`: no error; a string: a substring of the error.
    #[serde(default, deserialize_with = "some")]
    pub error: Option<Option<String>>,
    /// Requests `rt.sendRequest` made, in order (compared like `request`).
    #[serde(default)]
    pub sent: Option<Vec<ScriptRequest>>,
    /// The run took at most this long (wall clock, milliseconds).
    #[serde(default)]
    pub max_ms: Option<u64>,
    /// The run took at least this long.
    #[serde(default)]
    pub min_ms: Option<u64>,
}

/// Distinguishes a missing key (`None`) from `null` (`Some(None)`).
fn some<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectTest {
    pub name: String,
    pub passed: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectLog {
    pub level: String,
    pub text: String,
}

/// Parses every embedded case; problems as messages.
pub fn all() -> (Vec<(String, Case)>, Vec<String>) {
    let mut cases = Vec::new();
    let mut problems = Vec::new();
    for (path, json) in files() {
        match serde_json::from_str::<Case>(json) {
            Ok(case) => cases.push(((*path).to_owned(), case)),
            Err(e) => problems.push(format!("{path}: {e}")),
        }
    }
    (cases, problems)
}

/// Canned answers for `rt.sendRequest`, recording what was sent.
struct Canned {
    answers: Vec<CannedSend>,
    sent: std::cell::RefCell<Vec<ScriptRequest>>,
}

impl ScriptSender for Canned {
    fn send<'a>(
        &'a self,
        request: ScriptRequest,
    ) -> LocalBoxFuture<'a, Result<ScriptResponse, String>> {
        Box::pin(async move {
            self.sent.borrow_mut().push(request.clone());
            let answer = self.answers.iter().find(|a| {
                a.url == request.url
                    && a.method
                        .as_ref()
                        .is_none_or(|m| m.eq_ignore_ascii_case(&request.method))
            });
            let Some(answer) = answer else {
                return Err(format!(
                    "no canned answer for {} {}",
                    request.method, request.url
                ));
            };
            if answer.delay_ms > 0 {
                crate::timer::sleep(web_time::Duration::from_millis(answer.delay_ms)).await;
            }
            if let Some(e) = &answer.error {
                return Err(e.clone());
            }
            answer
                .response
                .as_ref()
                .ok_or_else(|| "canned answer without response".to_owned())?
                .to_response()
        })
    }
}

/// Runs one case; the failures as messages (empty: passed).
pub async fn run(case: &Case) -> Vec<String> {
    let mut failures = Vec::new();
    let response = match case.response.as_ref().map(CaseResponse::to_response) {
        None => None,
        Some(Ok(r)) => Some(r),
        Some(Err(e)) => return vec![format!("bad response: {e}")],
    };
    let info = case.info.clone().unwrap_or_default();
    let limits = Limits {
        time_ms: case
            .limits
            .and_then(|l| l.time_ms)
            .unwrap_or(Limits::default().time_ms),
        memory_bytes: case
            .limits
            .and_then(|l| l.memory_bytes)
            .unwrap_or(Limits::default().memory_bytes),
    };
    let sender = Canned {
        answers: case.send.clone(),
        sent: Default::default(),
    };
    let started = web_time::Instant::now();
    let outcome = Sandbox::new(limits)
        .run(ScriptCall {
            phase: case.phase,
            scripts: case.scripts.clone(),
            request: case.request.clone(),
            response,
            variables: case
                .variables
                .iter()
                .map(|v| ScriptVar {
                    name: v.name.clone(),
                    value: v.value.clone(),
                    source: v.source.clone(),
                    secret: v.secret,
                })
                .collect(),
            info: ScriptInfo {
                request_path: info.request_path,
                request_name: info.request_name,
                collection_name: info.collection_name,
                environment: info.environment,
                iteration: info.iteration,
            },
            sender: Some(&sender),
        })
        .await;
    let elapsed = started.elapsed().as_millis() as u64;
    compare(
        case,
        &outcome,
        &sender.sent.borrow(),
        elapsed,
        &mut failures,
    );
    failures
}

fn compare(
    case: &Case,
    got: &ScriptOutcome,
    sent: &[ScriptRequest],
    elapsed_ms: u64,
    f: &mut Vec<String>,
) {
    let want = &case.expect;
    if let Some(request) = &want.request
        && request != &got.request
    {
        f.push(format!("request: want {request:?}, got {:?}", got.request));
    }
    if let Some(changes) = &want.var_changes {
        let got_json = serde_json::to_value(&got.var_changes).unwrap_or_default();
        if got_json != Value::Array(changes.clone()) {
            f.push(format!(
                "var_changes: want {}, got {got_json}",
                Value::Array(changes.clone())
            ));
        }
    }
    if let Some(tests) = &want.tests {
        if tests.len() != got.tests.len() {
            f.push(format!(
                "tests: want {} ({:?}), got {} ({:?})",
                tests.len(),
                tests.iter().map(|t| &t.name).collect::<Vec<_>>(),
                got.tests.len(),
                got.tests
            ));
        }
        for (w, g) in tests.iter().zip(&got.tests) {
            if w.name != g.name || w.passed != g.passed {
                f.push(format!(
                    "test {:?}: want passed={}, got {:?} passed={} error={:?}",
                    w.name, w.passed, g.name, g.passed, g.error
                ));
            }
            if let Some(e) = &w.error
                && !g.error.as_deref().unwrap_or("").contains(e.as_str())
            {
                f.push(format!(
                    "test {:?}: want error containing {e:?}, got {:?}",
                    w.name, g.error
                ));
            }
            if let Some(o) = &w.origin
                && o != &g.origin
            {
                f.push(format!(
                    "test {:?}: want origin {o:?}, got {:?}",
                    w.name, g.origin
                ));
            }
        }
    }
    if let Some(logs) = &want.logs {
        let got_logs: Vec<(String, String)> = got
            .logs
            .iter()
            .map(|l| {
                let level = serde_json::to_value(l.level)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                (level, l.text.clone())
            })
            .collect();
        let want_logs: Vec<(String, String)> = logs
            .iter()
            .map(|l| (l.level.clone(), l.text.clone()))
            .collect();
        if got_logs != want_logs {
            f.push(format!("logs: want {want_logs:?}, got {got_logs:?}"));
        }
    }
    if let Some(flow) = &want.flow {
        let got_flow = serde_json::to_value(&got.flow).unwrap_or_default();
        if &got_flow != flow {
            f.push(format!("flow: want {flow}, got {got_flow}"));
        }
    }
    match (&want.error, &got.error) {
        (None, _) => {}
        (Some(None), None) => {}
        (Some(None), Some(e)) => f.push(format!("error: want none, got {e:?}")),
        (Some(Some(w)), None) => f.push(format!("error: want {w:?}, got none")),
        (Some(Some(w)), Some(g)) => {
            if !g.contains(w.as_str()) {
                f.push(format!("error: want {w:?} in {g:?}"));
            }
        }
    }
    if let Some(want_sent) = &want.sent
        && want_sent.as_slice() != sent
    {
        f.push(format!("sent: want {want_sent:?}, got {sent:?}"));
    }
    if let Some(max) = want.max_ms
        && elapsed_ms > max
    {
        f.push(format!("took {elapsed_ms} ms, want at most {max} ms"));
    }
    if let Some(min) = want.min_ms
        && elapsed_ms < min
    {
        f.push(format!("took {elapsed_ms} ms, want at least {min} ms"));
    }
}

/// Runs every case (skipping marked ones); one line per case, panics with the failures.
pub async fn run_all(prefix: &str, print: impl Fn(&str)) {
    let (cases, mut problems) = all();
    assert!(
        problems.is_empty(),
        "invalid case files:\n{}",
        problems.join("\n")
    );
    let mut by_file: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut ran = 0;
    // `RT_CASE=<part of a path>` runs only matching cases (natively; debugging).
    let only = option_env_case();
    for (path, case) in &cases {
        if !path.starts_with(prefix) || only.as_ref().is_some_and(|o| !path.contains(o.as_str())) {
            continue;
        }
        if let Some(reason) = &case.skip {
            print(&format!("SKIP {path}: {reason}"));
            continue;
        }
        let failures = run(case).await;
        ran += 1;
        if failures.is_empty() {
            print(&format!("ok   {path}"));
        } else {
            print(&format!(
                "FAIL {path} ({})\n  {}",
                case.name,
                failures.join("\n  ")
            ));
            by_file.insert(path, failures);
        }
    }
    for (path, failures) in by_file {
        problems.push(format!("{path}: {}", failures.join("; ")));
    }
    assert!(
        problems.is_empty(),
        "{} of {ran} cases failed:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

fn option_env_case() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("RT_CASE").ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

fn base64(text: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
    {
        let v = ALPHABET
            .iter()
            .position(|&a| a == c)
            .ok_or_else(|| format!("invalid base64 character {:?}", c as char))?;
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}
