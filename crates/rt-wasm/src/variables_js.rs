// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Variable completion in the web shell (wave 14), over `rt_app::variables`:
//!
//! ```text
//! variables(path: string, input: SendInput) -> VariableScope
//! defineVariable(folder: string, variable: Variable)
//! ```
//!
//! `input.secrets` only serve to mask values; secret values are never returned.

use rt_app::SendInput;
use rt_core::model::Variable;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    pub fn variables(&self, path: &str, input: JsValue) -> Result<JsValue, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let scope = self
            .workspace()
            .variables(path, &input)
            .map_err(app_error)?;
        to_js(&scope)
    }

    #[wasm_bindgen(js_name = defineVariable)]
    pub fn define_variable(&self, folder: &str, variable: JsValue) -> Result<(), JsValue> {
        let variable: Variable = from_js(&variable, "variable")?;
        self.workspace_mut()
            .define_variable(folder, &variable)
            .map_err(app_error)
    }
}
