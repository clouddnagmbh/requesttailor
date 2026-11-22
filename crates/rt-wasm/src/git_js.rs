// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Git helpers for the web shell, which runs Git itself (isomorphic-git): the shared line
//! diff, so the browser shows exactly the desktop's diff. Owner: wave 4, strand A2.

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use crate::workspace_js::to_js;

/// `GitDiff` of `old` → `new` (`null` for a missing side), as `rt_app::git::text_diff`.
#[wasm_bindgen(js_name = textDiff)]
pub fn text_diff(
    path: &str,
    old: Option<Uint8Array>,
    new: Option<Uint8Array>,
) -> Result<JsValue, JsValue> {
    let old = old.map(|a| a.to_vec());
    let new = new.map(|a| a.to_vec());
    to_js(&rt_app::git::text_diff(
        path,
        old.as_deref(),
        new.as_deref(),
    ))
}
