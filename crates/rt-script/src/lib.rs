// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-script`: the script sandbox (wave 7). Runs `before-request`, `after-response` and
//! `tests` scripts for `rt_core::send::send_with` through [`rt_core::services::ScriptRunner`].
//!
//! - Engine (strand A): QuickJS-ng compiled to WebAssembly (`engine/`, rebuilt reproducibly
//!   by `scripts/build-script-engine.mjs`), run in wasmi like plugins, so native, browser and
//!   iOS behave identically. Fuel plus the wall clock are the time limit, the store's memory
//!   limit the memory limit. No host functions except the script API (`api.d.ts`): no files,
//!   no network except `rt.sendRequest` through the core's transport. See [`engine`].
//! - JavaScript: `js/runtime` builds the API (strand A), `js/shims` (`bru`, `req`, `res`,
//!   `pm`, `expect`) run after it in file-name order, `js/modules/<name>.js` are what
//!   `require(name)` returns (strand B). All embedded at build time.
//! - Every behaviour has a case under `tests/cases` that runs natively
//!   (`cargo test -p rt-script --test cases`) and in the WASM core (`rt-wasm`'s browser
//!   tests), through [`cases`] (feature `test-cases`).
//! - The engine starts lazily, on the first run (collections without scripts never start
//!   it); [`warm_up`] starts it ahead of time.

use rt_core::script::{
    Flow, ScriptCall, ScriptLog, ScriptOutcome, ScriptRequest, ScriptSender, TestResult, VarChange,
    phase_name,
};
use rt_core::services::{LocalBoxFuture, ScriptRunner};
use serde::Deserialize;
use web_time::Instant;

#[cfg(feature = "test-cases")]
pub mod cases;
/// The script debugger (wave 19, strand Y).
pub mod debug;
mod engine;
mod timer;

pub use engine::FUEL_PER_MS;

/// Limits per phase and request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock budget in milliseconds, `rt.sleep` and `rt.sendRequest` included. The
    /// engine also has a fuel limit derived from it, so a busy loop stops on every platform.
    pub time_ms: u64,
    /// Memory of the engine instance in bytes.
    pub memory_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            time_ms: 5_000,
            memory_bytes: 64 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// "Script stopped: time limit of 5 s exceeded".
    pub fn time_message(&self) -> String {
        let t = if self.time_ms.is_multiple_of(1000) {
            format!("{} s", self.time_ms / 1000)
        } else {
            format!("{} ms", self.time_ms)
        };
        format!("Script stopped: time limit of {t} exceeded")
    }

    /// "Script stopped: memory limit of 64 MiB exceeded".
    pub fn memory_message(&self) -> String {
        let mib = self.memory_bytes as f64 / (1024.0 * 1024.0);
        let m = if self.memory_bytes.is_multiple_of(1024 * 1024) {
            format!("{} MiB", self.memory_bytes >> 20)
        } else {
            format!("{mib:.1} MiB")
        };
        format!("Script stopped: memory limit of {m} exceeded")
    }
}

/// The script runner. Cheap to keep; each [`ScriptRunner::run`] gets a fresh engine instance.
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    pub limits: Limits,
}

impl Sandbox {
    pub fn new(limits: Limits) -> Self {
        Self { limits }
    }

    async fn run_phase(&self, call: ScriptCall<'_>) -> ScriptOutcome {
        let input = input_of(&call);
        let host = Host {
            sender: call.sender,
        };
        let r = engine::run(&input, self.limits.time_ms, self.limits.memory_bytes, &host).await;
        self.outcome_of(r)
    }

    /// The outcome of a finished engine run (also for a debug run).
    pub(crate) fn outcome_of(&self, r: Result<Vec<u8>, engine::Failure>) -> ScriptOutcome {
        match r {
            Ok(reply) => self.outcome(&reply),
            Err(failure) => ScriptOutcome {
                error: Some(match failure {
                    engine::Failure::Time => self.limits.time_message(),
                    engine::Failure::Memory => self.limits.memory_message(),
                    engine::Failure::Stack => {
                        "Script stopped: too much recursion (stack overflow)".into()
                    }
                    engine::Failure::Engine(m) => m,
                }),
                ..ScriptOutcome::default()
            },
        }
    }

    pub(crate) fn outcome(&self, reply: &[u8]) -> ScriptOutcome {
        let out: Out = match serde_json::from_slice(reply) {
            Ok(out) => out,
            Err(e) => {
                return ScriptOutcome {
                    error: Some(format!("the script engine gave an invalid answer: {e}")),
                    ..ScriptOutcome::default()
                };
            }
        };
        let error = match out.stopped.as_deref() {
            Some("time") => Some(self.limits.time_message()),
            Some("memory") => Some(self.limits.memory_message()),
            _ => out.error,
        };
        ScriptOutcome {
            request: out.request,
            var_changes: out.var_changes,
            tests: out.tests,
            logs: out.logs,
            flow: out.flow,
            error,
        }
    }
}

impl ScriptRunner for Sandbox {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        Box::pin(self.run_phase(call))
    }
}

/// Starts the engine now instead of on the first script (e.g. right after a collection with
/// scripts was opened). Cheap once started.
pub async fn warm_up() -> Result<(), String> {
    engine::warm_up().await
}

/// What `rt_run` answers (`js/runtime/00-runtime.js`, `finish`).
#[derive(Deserialize)]
struct Out {
    #[serde(default)]
    request: Option<ScriptRequest>,
    #[serde(default)]
    var_changes: Vec<VarChange>,
    #[serde(default)]
    tests: Vec<TestResult>,
    #[serde(default)]
    logs: Vec<ScriptLog>,
    #[serde(default)]
    flow: Flow,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    stopped: Option<String>,
}

/// The message `rt_run` gets.
pub(crate) fn input_of(call: &ScriptCall<'_>) -> Vec<u8> {
    let response = call.response.as_ref().map(|r| {
        serde_json::json!({
            "status": r.status,
            "headers": r.headers,
            "body": String::from_utf8_lossy(&r.body),
            "size": r.body.len(),
            "time_ms": r.time_ms,
            "url": r.url,
        })
    });
    let mut state = seed();
    let seed: Vec<u32> = (0..4)
        .map(|_| {
            state = splitmix(state);
            (state >> 32) as u32
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "phase": phase_name(call.phase),
        "scripts": call.scripts,
        "request": call.request,
        "response": response,
        "variables": call.variables,
        "info": call.info,
        "seed": seed,
    }))
    .expect("JSON")
}

/// Answers `rt.sendRequest` and `rt.sleep` for one run.
pub(crate) struct Host<'a> {
    pub(crate) sender: Option<&'a dyn ScriptSender>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum Op {
    Send { request: ScriptRequest },
    Sleep { ms: f64 },
}

const STOP: &[u8] = br#"{"stop":true}"#;

impl engine::HostCalls for Host<'_> {
    fn call<'a>(&'a self, message: Vec<u8>, deadline: Instant) -> LocalBoxFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let op: Op = match serde_json::from_slice(&message) {
                Ok(op) => op,
                Err(e) => return error_reply(&format!("invalid host call: {e}")),
            };
            match op {
                Op::Sleep { ms } => {
                    let wait = web_time::Duration::from_secs_f64((ms.max(0.0)) / 1000.0);
                    let until = Instant::now() + wait;
                    // Waiting past the time limit cannot end well: stop right away.
                    if until > deadline {
                        return STOP.to_vec();
                    }
                    timer::sleep_until(until).await;
                    b"{}".to_vec()
                }
                Op::Send { request } => {
                    let Some(sender) = self.sender else {
                        return error_reply("rt.sendRequest is not available here");
                    };
                    match timer::until(sender.send(request), deadline).await {
                        None => STOP.to_vec(),
                        Some(Err(e)) => error_reply(&e),
                        Some(Ok(r)) => serde_json::to_vec(&serde_json::json!({
                            "response": {
                                "status": r.status,
                                "headers": r.headers,
                                "body": String::from_utf8_lossy(&r.body),
                                "size": r.body.len(),
                                "time_ms": r.time_ms,
                                "url": r.url,
                            }
                        }))
                        .expect("JSON"),
                    }
                }
            }
        })
    }
}

fn error_reply(message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "error": message })).expect("JSON")
}

/// Entropy for `Math.random` and WASI `random_get` (not cryptographic): the clock and a
/// counter, mixed.
fn seed() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);
    let nanos = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let n = COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    splitmix(nanos ^ n)
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A minimal executor (tests and the native case runner).
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-cases")))]
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct Thread(std::thread::Thread);
    impl Wake for Thread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Thread(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits() {
        assert_eq!(Sandbox::default().limits.time_ms, 5_000);
        assert_eq!(
            Limits::default().time_message(),
            "Script stopped: time limit of 5 s exceeded"
        );
        assert_eq!(
            Limits {
                time_ms: 250,
                ..Limits::default()
            }
            .time_message(),
            "Script stopped: time limit of 250 ms exceeded"
        );
        assert_eq!(
            Limits::default().memory_message(),
            "Script stopped: memory limit of 64 MiB exceeded"
        );
    }
}
