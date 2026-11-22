// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand A: the semantic diff for the web shell (`rt_app::diffs` over two `[path, text]` file lists).
//!
//! Interface placeholder; the strand adds its `#[wasm_bindgen]` functions here. The web
//! shell's file declares their TypeScript types itself (as `apps/web/src/runner.ts` does), so
//! `apps/web/src/wasm.ts` stays untouched.
//!
//! ```text
//! semanticDiff(old: [path, text][], new: [path, text][]) -> CollectionDiff
//! ```
//!
//! `old` and `new` are the collection's files on both sides (an empty list: the collection did
//! not exist there). The result is `rt_app::diffs::diff`, masked, the same as the desktop's and
//! `rt diff`'s. Throws a `BackendError`-shaped error when a side is not readable as a
//! collection.

use wasm_bindgen::prelude::*;

use crate::workspace_js::{from_js, to_js};

fn file_set(files: &JsValue, what: &str) -> Result<rt_format::FileSet, JsValue> {
    let files: Vec<(String, String)> = from_js(files, what)?;
    Ok(rt_format::FileSet {
        files: files
            .into_iter()
            .map(|(p, t)| (p, t.into_bytes()))
            .collect(),
    })
}

#[wasm_bindgen(js_name = semanticDiff)]
pub fn semantic_diff(old: JsValue, new: JsValue) -> Result<JsValue, JsValue> {
    let (old, new) = (file_set(&old, "old files")?, file_set(&new, "new files")?);
    match rt_app::diffs::diff(&old, &new) {
        Ok(diff) => to_js(&diff),
        Err(e) => Err(crate::workspace_js::app_error(e)),
    }
}
