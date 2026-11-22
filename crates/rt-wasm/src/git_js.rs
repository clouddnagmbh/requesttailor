// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Git helpers for the web shell, which runs Git itself (isomorphic-git): the shared line
//! diff, so the browser shows exactly the desktop's diff. Owner: wave 4, strand A2. Wave 5,
//! strand E: the secret scanner before a commit (`scanSecrets`).

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

/// `SecretFinding[]` of `files` (`[path, text][]`, as on disk), as `rt_app::secret_scan::scan`:
/// the web shell's `GitApi.scan`, and the check `GitApi.commit` runs first (wave 5, strand E).
#[wasm_bindgen(js_name = scanSecrets)]
pub fn scan_secrets(files: JsValue) -> Result<JsValue, JsValue> {
    let files: Vec<(String, String)> = crate::workspace_js::from_js(&files, "files")?;
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    to_js(&rt_app::secret_scan::scan(&refs))
}

/// The message of a refused commit for `findings` (`SecretFinding[]`, not empty), as the
/// desktop's `AppError::SecretsFound` (`rt_app::secret_scan::refusal`).
#[wasm_bindgen(js_name = secretRefusal)]
pub fn secret_refusal(findings: JsValue) -> Result<String, JsValue> {
    let findings: Vec<rt_app::secret_scan::SecretFinding> =
        crate::workspace_js::from_js(&findings, "findings")?;
    if findings.is_empty() {
        return Err(js_sys::TypeError::new("findings must not be empty").into());
    }
    Ok(rt_app::secret_scan::refusal(&findings))
}
