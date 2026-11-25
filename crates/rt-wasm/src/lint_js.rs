// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand F: the web shell's `Workspace` methods for `Backend.lint`, over
//! `rt_app::lint`. The lint works on the model in memory and sends nothing.
//!
//! ```text
//! lintRun() -> LintReport
//! lintSettings() -> LintSettings
//! lintSetSettings(settings: LintSettings)
//! lintIgnore(path: string, rule: LintRule)
//! ```

use rt_app::lint::{self, LintRule, LintSettings};
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = lintRun)]
    pub fn lint_run(&self) -> Result<JsValue, JsValue> {
        to_js(&lint::run(&self.workspace()))
    }

    #[wasm_bindgen(js_name = lintSettings)]
    pub fn lint_settings(&self) -> Result<JsValue, JsValue> {
        to_js(&lint::settings(&self.workspace()))
    }

    #[wasm_bindgen(js_name = lintSetSettings)]
    pub fn lint_set_settings(&self, settings: JsValue) -> Result<(), JsValue> {
        let settings: LintSettings = from_js(&settings, "settings")?;
        lint::set_settings(&mut self.workspace_mut(), &settings).map_err(app_error)
    }

    #[wasm_bindgen(js_name = lintIgnore)]
    pub fn lint_ignore(&self, path: &str, rule: JsValue) -> Result<(), JsValue> {
        let rule: LintRule = from_js(&rule, "rule")?;
        lint::ignore(&mut self.workspace_mut(), path, rule).map_err(app_error)
    }
}
