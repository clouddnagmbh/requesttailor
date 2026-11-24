// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand C: the web shell's calls for `Backend.share`, over `rt_app::share`.
//! Nothing here makes a request.
//!
//! ```text
//! Workspace.shareLink(path, options: ShareOptions) -> ShareLink
//! Workspace.shareCopyAs(path, kind: CopyKind, options: CopyOptions) -> CopiedAs
//! shareOpen(input: string) -> SharePayload      // a link, a fragment or an `rt:` payload
//! shareQr(link: string) -> string               // SVG
//! shareLooksShared(text: string) -> boolean     // whether a paste is such a text
//! ```

use rt_app::share::{self, CopyKind, CopyOptions, ShareOptions};
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = shareLink)]
    pub fn share_link(&self, path: &str, options: JsValue) -> Result<JsValue, JsValue> {
        let options: ShareOptions = from_js(&options, "options")?;
        let link = share::link(&self.workspace(), path, &options).map_err(app_error)?;
        to_js(&link)
    }

    #[wasm_bindgen(js_name = shareCopyAs)]
    pub fn share_copy_as(
        &self,
        path: &str,
        kind: JsValue,
        options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let kind: CopyKind = from_js(&kind, "kind")?;
        let options: CopyOptions = from_js(&options, "options")?;
        let copied = share::copy_as(&self.workspace(), path, kind, &options).map_err(app_error)?;
        to_js(&copied)
    }
}

#[wasm_bindgen(js_name = shareOpen)]
pub fn share_open(input: &str) -> Result<JsValue, JsValue> {
    to_js(&share::open(input).map_err(app_error)?)
}

#[wasm_bindgen(js_name = shareQr)]
pub fn share_qr(link: &str) -> Result<String, JsValue> {
    share::qr(link).map_err(app_error)
}

#[wasm_bindgen(js_name = shareLooksShared)]
pub fn share_looks_shared(text: &str) -> bool {
    share::looks_shared(text)
}
