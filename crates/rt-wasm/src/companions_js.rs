// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Companion files for extensions in the web shell (wave 13), over `rt_app::companions`:
//!
//! ```text
//! companionList(prefix: string) -> string[]
//! companionRead(path: string) -> string | null
//! companionStage(path: string, text: string)
//! ```

use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = companionList)]
    pub fn companion_list(&self, prefix: &str) -> Result<JsValue, JsValue> {
        to_js(&rt_app::companions::list(&self.workspace(), prefix))
    }

    #[wasm_bindgen(js_name = companionRead)]
    pub fn companion_read(&self, path: &str) -> Option<String> {
        rt_app::companions::read(&self.workspace(), path)
    }

    #[wasm_bindgen(js_name = companionStage)]
    pub fn companion_stage(&self, path: &str, text: &str) -> Result<(), JsValue> {
        rt_app::companions::stage(&mut self.workspace_mut(), path, text).map_err(app_error)
    }
}
