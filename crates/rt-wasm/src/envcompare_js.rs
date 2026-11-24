// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand A: comparing environments (`rt_app::envcompare`) over the open `Workspace`,
//! with the same transports and services as `Workspace.send`. The web shell's
//! `src/envcompare.ts` declares the TypeScript type.
//!
//! ```text
//! Workspace.compareEnvironments(request, inputs, companion, services, cancelled)
//!     -> Promise<EnvComparison>
//! ```
//!
//! `request` is an `EnvCompareRequest`; `inputs` has one `SendInput` per environment of the
//! request (environment name, runtime variables, secret values of that environment, in the
//! same order), `services` one send-services object (or `null`) per environment, `cancelled()`
//! is polled before each request. Rejects with the usual shell errors.

use js_sys::{Array, Function, Promise};
use rt_app::SendInput;
use rt_app::envcompare::{EnvCompareRequest, EnvRun, compare_environments};
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::services_js::SendServices;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Runs the reading requests in every environment and compares; see the module
    /// documentation.
    #[wasm_bindgen(js_name = compareEnvironments)]
    pub fn compare_environments_js(
        &self,
        request: JsValue,
        inputs: JsValue,
        companion: JsValue,
        services: JsValue,
        cancelled: Function,
    ) -> Result<Promise, JsValue> {
        let request: EnvCompareRequest = from_js(&request, "request")?;
        let inputs: Vec<SendInput> = from_js(&inputs, "inputs")?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let parts: Vec<SendServices> = Array::from(&services)
            .iter()
            .map(|s| SendServices::from_js(&s))
            .collect::<Result<_, _>>()?;
        if parts.len() != inputs.len() {
            return Err(JsValue::from_str(
                "compareEnvironments needs one input and one services object per environment",
            ));
        }
        let snapshots: Vec<_> = inputs
            .into_iter()
            .map(|input| self.workspace().run_snapshot(input))
            .collect();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let runs: Vec<EnvRun> = snapshots
                .into_iter()
                .zip(&parts)
                .map(|(snapshot, parts)| EnvRun {
                    snapshot,
                    services: parts.services(),
                })
                .collect();
            let options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let stop = || {
                cancelled
                    .call0(&JsValue::NULL)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
            };
            let result = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    compare_environments(&runs, &request, &t, &options, &stop).await
                }
                None => {
                    let t = FetchTransport::new();
                    compare_environments(&runs, &request, &t, &options, &stop).await
                }
            };
            to_js(&result.map_err(app_error)?)
        }))
    }
}
