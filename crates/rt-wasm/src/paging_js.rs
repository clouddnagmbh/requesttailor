// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand C: the web shell's `Workspace` methods for `Backend.paging`, over
//! `rt_app::paging`. Pages go out with the same transports and services as `Workspace.send`
//! (the companion when it is on), so the send guard answers for every page. The shell passes
//! the paging rules of the active plugins (`PluginPaging[]`, from their manifests) because the
//! plugins live on the JavaScript side.
//!
//! ```text
//! pagingRule(path, responseId | null, plugins) -> EffectivePaging | null
//! pagingSetRule(path, spec | null, scope)
//! pagingRun(run, plugins, input, companion, services, cancelled) -> Promise<PagedResult>
//! ```
//!
//! `run` is a `PagingRun` (its `input` is ignored: `input` is the `SendInput` with the
//! environment's secret values, which never leave the core); `cancelled()` is polled between
//! pages.

use js_sys::{Function, Promise};
use rt_app::paging::rules::{self, MatchContext};
use rt_app::paging::{
    PagingMode, PagingRuleScope, PagingRun, PagingSpec, PluginPaging, previous_page,
    run_on_snapshot,
};
use rt_app::{AppError, SendInput};
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::services_js::SendServices;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

fn plugins_from(value: &JsValue) -> Result<Vec<PluginPaging>, JsValue> {
    if value.is_null() || value.is_undefined() {
        return Ok(Vec::new());
    }
    from_js(value, "plugins")
}

fn id_from(value: &JsValue) -> Option<u64> {
    value.as_f64().filter(|v| *v >= 0.0).map(|v| v as u64)
}

impl WorkspaceJs {
    fn paging_context(&self, path: &str, response_id: Option<u64>) -> Option<MatchContext> {
        let ws = self.workspace();
        if let Some(report) = response_id.and_then(|id| self.responses().report(id).ok().cloned()) {
            return Some(MatchContext {
                url: report
                    .request
                    .as_ref()
                    .map(|r| r.url.clone())
                    .unwrap_or_default(),
                content_type: report.response.as_ref().and_then(|r| {
                    r.headers
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
                        .map(|(_, v)| v.clone())
                }),
            });
        }
        ws.request(path).ok().map(|spec| MatchContext {
            url: spec.url,
            content_type: None,
        })
    }
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = pagingRule)]
    pub fn paging_rule(
        &self,
        path: &str,
        response_id: JsValue,
        plugins: JsValue,
    ) -> Result<JsValue, JsValue> {
        let plugins = plugins_from(&plugins)?;
        let context = self.paging_context(path, id_from(&response_id));
        let found = rules::effective(&self.workspace(), path, &plugins, context.as_ref())
            .map_err(app_error)?;
        to_js(&found)
    }

    #[wasm_bindgen(js_name = pagingSetRule)]
    pub fn paging_set_rule(
        &self,
        path: &str,
        spec: JsValue,
        scope: JsValue,
    ) -> Result<(), JsValue> {
        let spec: Option<PagingSpec> = if spec.is_null() || spec.is_undefined() {
            None
        } else {
            Some(from_js(&spec, "spec")?)
        };
        let scope: PagingRuleScope = from_js(&scope, "scope")?;
        rules::set(&mut self.workspace_mut(), path, spec.as_ref(), scope).map_err(app_error)
    }

    #[wasm_bindgen(js_name = pagingRun)]
    pub fn paging_run(
        &self,
        run: JsValue,
        plugins: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        cancelled: Function,
    ) -> Result<Promise, JsValue> {
        let run: PagingRun = from_js(&run, "run")?;
        let input: SendInput = from_js(&input, "input")?;
        let plugins = plugins_from(&plugins)?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let parts = SendServices::from_js(&services)?;
        let context = self.paging_context(&run.path, run.response_id);
        let rule = rules::effective(&self.workspace(), &run.path, &plugins, context.as_ref())
            .map_err(app_error)?
            .ok_or_else(|| app_error(AppError::Shell("This request has no paging rule.".into())))?;
        let previous = match run.mode {
            PagingMode::Next => {
                let id = run.response_id.ok_or_else(|| {
                    app_error(AppError::Shell("There is no page to continue from.".into()))
                })?;
                Some(previous_page(&self.responses(), id).map_err(app_error)?)
            }
            PagingMode::All => None,
        };
        let snapshot = self.workspace().run_snapshot(input);
        let this = self.handle();
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
            let paged = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    run_on_snapshot(
                        &snapshot, &run, &rule.spec, previous, &t, &options, &services, &stop,
                    )
                    .await
                }
                None => {
                    let t = FetchTransport::new();
                    run_on_snapshot(
                        &snapshot, &run, &rule.spec, previous, &t, &options, &services, &stop,
                    )
                    .await
                }
            }
            .map_err(app_error)?;
            let result = paged.into_result(&mut this.responses_mut());
            to_js(&result)
        }))
    }
}
