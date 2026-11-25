// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand E: the minimal failing request (`rt_app::shrink`) over the open `Workspace`,
//! with the same transports and services as `Workspace.send` (so the send guard answers for
//! every probe). The web shell's `src/shrink.ts` declares the TypeScript types.
//!
//! ```text
//! Workspace.shrink(request, input, companion, services, cancelled) -> Promise<ShrinkResult>
//! Workspace.shrinkPredicate(responseId) -> ShrinkPredicate
//! ```
//!
//! `request` is a `ShrinkRequest`; `input` is the `SendInput` of the environment (secret values
//! of that environment, which never leave the core), `services` one send-services object (or
//! `null`), `cancelled()` is polled before each probe. `shrinkPredicate` says what failed in
//! the kept response (rejects when the answer did not fail).

use js_sys::{Function, Promise};
use rt_app::SendInput;
use rt_app::shrink::{ShrinkRequest, describe_failure, shrink};
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::services_js::SendServices;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Shrinks the request; see the module documentation.
    #[wasm_bindgen(js_name = shrink)]
    pub fn shrink_js(
        &self,
        request: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        cancelled: Function,
    ) -> Result<Promise, JsValue> {
        let request: ShrinkRequest = from_js(&request, "request")?;
        let input: SendInput = from_js(&input, "input")?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let parts = SendServices::from_js(&services)?;
        let snapshot = self.workspace().run_snapshot(input);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
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
            let services = parts.services();
            let result = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    shrink(&snapshot, &request, &t, &options, &services, &stop).await
                }
                None => {
                    let t = FetchTransport::new();
                    shrink(&snapshot, &request, &t, &options, &services, &stop).await
                }
            };
            to_js(&result.map_err(app_error)?)
        }))
    }

    /// What failed in the kept response `id` as a predicate.
    #[wasm_bindgen(js_name = shrinkPredicate)]
    pub fn shrink_predicate(&self, id: f64) -> Result<JsValue, JsValue> {
        let responses = self.responses();
        let report = responses.report(id as u64).map_err(app_error)?;
        let predicate = describe_failure(report).ok_or_else(|| {
            app_error(rt_app::AppError::Shell(
                "The last answer did not fail.".into(),
            ))
        })?;
        to_js(&predicate)
    }
}
