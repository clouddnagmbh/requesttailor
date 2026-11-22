// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection runner in the browser (wave 7, strand D): `rt_core::runner` over the open
//! `Workspace` (`rt_app::runner::RunSnapshot`), with the same transports and services as
//! `Workspace.send`.
//!
//! ```text
//! Workspace.runPlan(folder?) -> string[]          request paths in run order
//! Workspace.run(plan, input, companion, services, onEvent, cancelled) -> Promise<RunSummary>
//! parseData(name, data: Uint8Array) -> DataRow[]   throws an Error named `DataError`
//! ```
//!
//! `plan` is a `RunPlan`, `input` an `rt_app::SendInput` (with the secret values), `companion`
//! and `services` as for `Workspace.send`. `onEvent(event)` is called with every `RunEvent`
//! while the run goes on; `cancelled()` is polled before every request and during the delay
//! (a truthy result stops the run). The promise resolves with the summary, also when the run
//! was stopped. Variable changes stay in the run. Step events carry a body preview only
//! (`rt_app::runner::for_view`).

use js_sys::{Function, Promise, Uint8Array};
use rt_app::SendInput;
use rt_core::runner::{RunPlan, parse_data};
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Request paths below `folder` (`null`/`undefined`/`""`: the whole collection).
    #[wasm_bindgen(js_name = runPlan)]
    pub fn run_plan(&self, folder: Option<String>) -> Result<JsValue, JsValue> {
        to_js(&rt_core::runner::plan(
            self.workspace().collection(),
            folder.as_deref(),
        ))
    }

    /// Runs `plan`; see the module documentation.
    #[wasm_bindgen(js_name = run)]
    pub fn run_js(
        &self,
        plan: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        #[wasm_bindgen(js_name = onEvent)] on_event: Function,
        cancelled: Function,
    ) -> Result<Promise, JsValue> {
        let plan: RunPlan = from_js(&plan, "plan")?;
        let input: SendInput = from_js(&input, "input")?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let snapshot = self.workspace().run_snapshot(input);
        let parts = crate::services_js::SendServices::from_js(&services)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let services = parts.services();
            let mut emit = |event| {
                if let Ok(value) = to_js(&rt_app::runner::for_view(event)) {
                    // A throwing listener must not stop the run.
                    let _ = on_event.call1(&JsValue::NULL, &value);
                }
            };
            let stop = || {
                cancelled
                    .call0(&JsValue::NULL)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
            };
            let summary = match companion {
                Some(c) => {
                    let transport = CompanionTransport::new(c.url, c.token);
                    snapshot
                        .run(&plan, &transport, &options, &services, &mut emit, &stop)
                        .await
                }
                None => {
                    let transport = FetchTransport::new();
                    snapshot
                        .run(&plan, &transport, &options, &services, &mut emit, &stop)
                        .await
                }
            };
            to_js(&summary)
        }))
    }
}

/// The rows of a CSV or JSON data file (`rt_core::runner::parse_data`).
#[wasm_bindgen(js_name = parseData)]
pub fn parse_data_js(name: &str, data: Uint8Array) -> Result<JsValue, JsValue> {
    match parse_data(name, &data.to_vec()) {
        Ok(rows) => to_js(&rows),
        Err(message) => {
            let err = js_sys::Error::new(&message);
            err.set_name("DataError");
            Err(err.into())
        }
    }
}
