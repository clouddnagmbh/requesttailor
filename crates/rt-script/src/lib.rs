// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-script`: the script sandbox (wave 7). Runs `before-request`, `after-response` and
//! `tests` scripts for `rt_core::send::send_with` through [`rt_core::services::ScriptRunner`].
//!
//! - Engine (strand A): QuickJS compiled to WebAssembly, run in the same wasmi interpreter as
//!   plugins, so native, browser and iOS behave identically. Fuel is the time limit, the
//!   wasmi memory limit the memory limit. No host functions except the script API
//!   (`api.d.ts`): no files, no network except `rt.sendRequest` through the core's
//!   transport.
//! - Shims (strand B): `bru.*`, `req`, `res`, `pm.*` on top of the API, by corpus frequency;
//!   coverage in `SHIM-COVERAGE.md`. Built-in modules (`crypto-js`, `uuid`, …) ditto.
//! - Every behaviour has a test that runs natively and in the WASM core (`rt-wasm`).

use rt_core::script::{ScriptCall, ScriptOutcome};
use rt_core::services::{LocalBoxFuture, ScriptRunner};

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

/// The script runner. Cheap to keep; each [`ScriptRunner::run`] gets a fresh engine instance.
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    pub limits: Limits,
}

impl Sandbox {
    pub fn new(limits: Limits) -> Self {
        Self { limits }
    }
}

impl ScriptRunner for Sandbox {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        // Interface stub (strand A): no engine yet.
        let _ = call;
        Box::pin(async {
            ScriptOutcome {
                error: Some("the script engine is not available yet".into()),
                ..ScriptOutcome::default()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits() {
        assert_eq!(Sandbox::default().limits.time_ms, 5_000);
    }
}
