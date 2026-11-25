// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand R: the web shell's methods for `Backend.security` over `rt_app::security`.
//! Nothing here sends anything; the answers carry rule, place and sentences, never a token.
//!
//! ```text
//! securityRules() -> SecurityRule[]                              // module function
//! securityIgnoredBy(settings: SecuritySettings) -> string[]      // ids the settings switch off
//! Workspace.securityCheck(responseId, deviceIgnored: string[], settings) -> SecurityFinding[]
//! ```
//!
//! The settings of this device are the shell's own (browser storage); the collection's list of
//! rules switched off is read from `opencollection.yml` here. The checks chip and tab get the
//! same rules through `inspectChecks` (the shell adds `securityIgnoredBy` to its list).

use rt_app::inspect;
use rt_app::security::{self, SecuritySettings};
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

/// The rules of the passive audit with category and severity.
#[wasm_bindgen(js_name = securityRules)]
pub fn security_rules() -> Result<JsValue, JsValue> {
    to_js(&security::rules())
}

/// The rule ids that the device settings switch off.
#[wasm_bindgen(js_name = securityIgnoredBy)]
pub fn security_ignored_by(settings: JsValue) -> Result<JsValue, JsValue> {
    let settings: SecuritySettings = from_js(&settings, "settings")?;
    to_js(&security::ignored_by(&settings))
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// The findings of the kept response `response_id`, minus the rules switched off on this
    /// device (`device_ignored`, `settings`) and in the collection.
    #[wasm_bindgen(js_name = securityCheck)]
    pub fn security_check(
        &self,
        response_id: f64,
        device_ignored: JsValue,
        settings: JsValue,
    ) -> Result<JsValue, JsValue> {
        let device: Vec<String> = from_js(&device_ignored, "deviceIgnored")?;
        let settings: SecuritySettings = from_js(&settings, "settings")?;
        let ignore = inspect::all_ignored(&self.workspace(), &device);
        let found = security::passive_of(&self.responses(), int(response_id)?, &ignore, &settings)
            .map_err(app_error)?;
        to_js(&found)
    }
}
