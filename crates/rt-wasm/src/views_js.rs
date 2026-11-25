// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand G: the web shell's `Workspace` methods for `Backend.views`, over
//! `rt_app::views`. The file `<request>.views.json` is staged like any companion file (never
//! written by itself); a view is evaluated on the kept response, on the Rust side.
//!
//! ```text
//! viewsList(path) -> ResponseView[]
//! viewsSave(path, view) -> ResponseView[]
//! viewsRemove(path, name) -> ResponseView[]
//! viewsApply(responseId, view) -> ViewResult
//! viewsLanguages() -> ViewLanguage[]
//! ```

use rt_app::views::{self, ResponseView};
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = viewsList)]
    pub fn views_list(&self, path: &str) -> Result<JsValue, JsValue> {
        to_js(&views::list(&self.workspace(), path).map_err(app_error)?)
    }

    #[wasm_bindgen(js_name = viewsSave)]
    pub fn views_save(&self, path: &str, view: JsValue) -> Result<JsValue, JsValue> {
        let view: ResponseView = from_js(&view, "view")?;
        to_js(&views::save(&mut self.workspace_mut(), path, view).map_err(app_error)?)
    }

    #[wasm_bindgen(js_name = viewsRemove)]
    pub fn views_remove(&self, path: &str, name: &str) -> Result<JsValue, JsValue> {
        to_js(&views::remove(&mut self.workspace_mut(), path, name).map_err(app_error)?)
    }

    #[wasm_bindgen(js_name = viewsApply)]
    pub fn views_apply(&self, response_id: f64, view: JsValue) -> Result<JsValue, JsValue> {
        let view: ResponseView = from_js(&view, "view")?;
        let id = int(response_id)?;
        to_js(&views::apply(&self.responses(), id, &view).map_err(app_error)?)
    }

    #[wasm_bindgen(js_name = viewsLanguages)]
    pub fn views_languages(&self) -> Result<JsValue, JsValue> {
        to_js(&views::languages())
    }
}
