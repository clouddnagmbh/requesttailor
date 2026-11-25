// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand W: API notebooks for the web shell over `rt_app::notebook`. A notebook is a
//! companion file of the open `Workspace` (staged until `save`); running sends its request
//! blocks with the same transports and services as `Workspace.send`, so the send guard answers
//! for each one. The web shell's `src/notebook.ts` declares the TypeScript type.
//!
//! ```text
//! Workspace.notebookList() -> NotebookEntry[]
//! Workspace.notebookRead(path) -> Notebook
//! Workspace.notebookStage(path, notebook)
//! Workspace.notebookRun(run, input, companion, services, cancelled) -> Promise<NotebookOutcome>
//! ```
//!
//! `input` is a `SendInput` (environment name, the secret values of that environment, which
//! never leave the core), `services` one send-services object (or `null`), `cancelled()` is
//! polled before each block. The web keeps no audit log. Outputs are returned, never stored.

use js_sys::Function;
use js_sys::Promise;
use rt_app::SendInput;
use rt_app::envcompare::EnvRun;
use rt_app::notebook::{Notebook, NotebookEnv, NotebookRun, run};
use rt_core::send::SendOptions;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::services_js::SendServices;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};
use crate::{BODY_LIMIT, Companion};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// The notebook files of the collection.
    #[wasm_bindgen(js_name = notebookList)]
    pub fn notebook_list(&self) -> Result<JsValue, JsValue> {
        to_js(&rt_app::notebook::list(&self.workspace()))
    }

    /// Reads one notebook.
    #[wasm_bindgen(js_name = notebookRead)]
    pub fn notebook_read(&self, path: &str) -> Result<JsValue, JsValue> {
        to_js(&rt_app::notebook::read(&self.workspace(), path).map_err(app_error)?)
    }

    /// Stages a notebook as the file `path`, unsaved.
    #[wasm_bindgen(js_name = notebookStage)]
    pub fn notebook_stage(&self, path: &str, notebook: JsValue) -> Result<(), JsValue> {
        let notebook: Notebook = from_js(&notebook, "notebook")?;
        rt_app::notebook::stage(&mut self.workspace_mut(), path, &notebook).map_err(app_error)
    }

    /// Runs the blocks of a notebook; see the module documentation.
    #[wasm_bindgen(js_name = notebookRun)]
    pub fn notebook_run(
        &self,
        request: JsValue,
        input: JsValue,
        companion: JsValue,
        services: JsValue,
        cancelled: Function,
    ) -> Result<Promise, JsValue> {
        let request: NotebookRun = from_js(&request, "run")?;
        let input: SendInput = from_js(&input, "input")?;
        let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
            None
        } else {
            Some(from_js(&companion, "companion")?)
        };
        let parts = SendServices::from_js(&services)?;
        let notebook =
            rt_app::notebook::read(&self.workspace(), &request.path).map_err(app_error)?;
        let snapshot = self.workspace().run_snapshot(input);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let env_run = EnvRun {
                snapshot,
                services: parts.services(),
            };
            let send_options = SendOptions {
                body_limit: BODY_LIMIT,
                iteration: None,
            };
            let stop = || {
                cancelled
                    .call0(&JsValue::NULL)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
            };
            let outcome = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    let env = NotebookEnv {
                        run: &env_run,
                        transport: &t,
                        options: &send_options,
                        caller: "app",
                        audit: &|_| {},
                        cancelled: &stop,
                    };
                    run(&env, &notebook, &request).await
                }
                None => {
                    let t = FetchTransport::new();
                    let env = NotebookEnv {
                        run: &env_run,
                        transport: &t,
                        options: &send_options,
                        caller: "app",
                        audit: &|_| {},
                        cancelled: &stop,
                    };
                    run(&env, &notebook, &request).await
                }
            };
            to_js(&outcome.map_err(app_error)?)
        }))
    }
}
