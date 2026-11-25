// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand B: the web shell's methods for `Backend.inspect` over `rt_app::inspect`.
//! Nothing here sends anything; a token value is returned only to the view that asked.
//!
//! ```text
//! inspectJwt(token: string) -> JwtInfo                       // module function, no session
//! Workspace.inspectTokens(path, input, responseId | null) -> FoundToken[]
//! Workspace.inspectChecks(responseId, deviceIgnored: string[]) -> Observation[]
//! Workspace.inspectIgnoreCollection(id: string)              // staged in opencollection.yml
//! Workspace.inspectCollectionIgnored() -> string[]
//! ```
//!
//! `input` is a `SendInput` (environment, runtime variables, secret values of that
//! environment); the shell's list of rules switched off on this device is its own
//! (`deviceIgnored`), the collection's is read from `opencollection.yml` here.

use rt_app::SendInput;
use rt_app::inspect;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

/// Reads a token for display; never verifies the signature.
#[wasm_bindgen(js_name = inspectJwt)]
pub fn inspect_jwt(token: &str) -> Result<JsValue, JsValue> {
    to_js(&inspect::jwt(token).map_err(app_error)?)
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// The tokens of the resolved request `path` and, with `response_id`, of that response.
    #[wasm_bindgen(js_name = inspectTokens)]
    pub fn inspect_tokens(
        &self,
        path: &str,
        input: JsValue,
        response_id: Option<f64>,
    ) -> Result<JsValue, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let mut found =
            inspect::request_tokens(&self.workspace(), path, input).map_err(app_error)?;
        if let Some(id) = response_id {
            let id = int(id)?;
            found.extend(inspect::response_tokens(&self.responses(), id).map_err(app_error)?);
        }
        to_js(&found)
    }

    /// The passive checks of the kept response `response_id`, minus the rules switched off on
    /// this device (`device_ignored`) and in the collection.
    #[wasm_bindgen(js_name = inspectChecks)]
    pub fn inspect_checks(
        &self,
        response_id: f64,
        device_ignored: JsValue,
    ) -> Result<JsValue, JsValue> {
        let device: Vec<String> = from_js(&device_ignored, "deviceIgnored")?;
        let ignore = inspect::all_ignored(&self.workspace(), &device);
        let found =
            inspect::checks_of(&self.responses(), int(response_id)?, &ignore).map_err(app_error)?;
        to_js(&found)
    }

    /// Switches a rule off for the collection (staged, unsaved).
    #[wasm_bindgen(js_name = inspectIgnoreCollection)]
    pub fn inspect_ignore_collection(&self, id: &str) -> Result<(), JsValue> {
        inspect::ignore_in_collection(&mut self.workspace_mut(), id).map_err(app_error)
    }

    /// The rule ids the collection switches off.
    #[wasm_bindgen(js_name = inspectCollectionIgnored)]
    pub fn inspect_collection_ignored(&self) -> Result<JsValue, JsValue> {
        to_js(&inspect::collection_ignored(&self.workspace()))
    }
}
