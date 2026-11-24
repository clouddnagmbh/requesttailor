// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand B: the web shell's `Workspace` methods for `Backend.snapshots`, over
//! `rt_app::snapshots`. The kept body still has the secret values of its send, so the shell
//! asks for the response's environment first and passes that environment's secret values
//! (only to mask them) to `freeze` and `check`:
//!
//! ```text
//! snapshotsEnvironment(responseId) -> string | null
//! snapshotsRead(path) -> Snapshot | null
//! snapshotsFreeze(path, responseId, options, secrets: string[]) -> SnapshotFreeze
//! snapshotsSetIgnore(path, ignore: string[]) -> Snapshot
//! snapshotsCheck(path, responseId, secrets: string[]) -> SnapshotCheck
//! snapshotsList() -> SnapshotEntry[]
//! ```

use rt_app::snapshots::{self, FreezeOptions};
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = snapshotsEnvironment)]
    pub fn snapshots_environment(&self, response_id: f64) -> Result<Option<String>, JsValue> {
        let id = int(response_id)?;
        snapshots::environment_of(&self.responses(), id).map_err(app_error)
    }

    #[wasm_bindgen(js_name = snapshotsRead)]
    pub fn snapshots_read(&self, path: &str) -> Result<JsValue, JsValue> {
        to_js(&snapshots::read(&self.workspace(), path).map_err(app_error)?)
    }

    #[wasm_bindgen(js_name = snapshotsFreeze)]
    pub fn snapshots_freeze(
        &self,
        path: &str,
        response_id: f64,
        options: JsValue,
        secrets: JsValue,
    ) -> Result<JsValue, JsValue> {
        let id = int(response_id)?;
        let options: FreezeOptions = from_js(&options, "options")?;
        let secrets: Vec<String> = from_js(&secrets, "secrets")?;
        let plan = snapshots::plan_freeze(
            &self.workspace(),
            &self.responses(),
            path,
            id,
            &options,
            &secrets,
        )
        .map_err(app_error)?;
        let done = snapshots::apply_freeze(&mut self.workspace_mut(), plan).map_err(app_error)?;
        to_js(&done)
    }

    #[wasm_bindgen(js_name = snapshotsSetIgnore)]
    pub fn snapshots_set_ignore(&self, path: &str, ignore: JsValue) -> Result<JsValue, JsValue> {
        let ignore: Vec<String> = from_js(&ignore, "ignore")?;
        let snapshot =
            snapshots::set_ignore(&mut self.workspace_mut(), path, ignore).map_err(app_error)?;
        to_js(&snapshot)
    }

    #[wasm_bindgen(js_name = snapshotsCheck)]
    pub fn snapshots_check(
        &self,
        path: &str,
        response_id: f64,
        secrets: JsValue,
    ) -> Result<JsValue, JsValue> {
        let id = int(response_id)?;
        let secrets: Vec<String> = from_js(&secrets, "secrets")?;
        let check = snapshots::check(&self.workspace(), &self.responses(), path, id, &secrets)
            .map_err(app_error)?;
        to_js(&check)
    }

    #[wasm_bindgen(js_name = snapshotsList)]
    pub fn snapshots_list(&self) -> Result<JsValue, JsValue> {
        to_js(&snapshots::list(&self.workspace()))
    }
}
