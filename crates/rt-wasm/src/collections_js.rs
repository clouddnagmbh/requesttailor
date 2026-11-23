// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collections in the sidebar for the web shell (wave 14, strand A), over
//! `rt_app::collections`: the one discovery rule, new collections and search across them.
//!
//! ```text
//! collectionsEnter(name: string, depth: number) -> boolean
//! collectionsDiscover(manifestDirs: string[]) -> { dirs: string[], truncated: boolean }
//! collectionsCheckDir(dir: string) -> string            // throws AppError (kind shell)
//! collectionsScaffold(spec: NewCollection) -> FileChange[]
//! collectionsMergeHits(perCollection: [string, SearchHit[]][], limit: number)
//!   -> CollectionSearchHit[]
//! ```

use rt_app::SearchHit;
use rt_app::collections::NewCollection;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{app_error, from_js, to_js};

/// Whether discovery enters the directory `name` at `depth` (the root's children: 1).
#[wasm_bindgen(js_name = collectionsEnter)]
pub fn collections_enter(name: &str, depth: usize) -> bool {
    rt_app::collections::enter(name, depth)
}

/// The collections of a root from the directories (relative, `""` = root) in which a walk
/// with `collectionsEnter` found an `opencollection.yml`.
#[wasm_bindgen(js_name = collectionsDiscover)]
pub fn collections_discover(manifest_dirs: JsValue) -> Result<JsValue, JsValue> {
    let dirs: Vec<String> = from_js(&manifest_dirs, "manifestDirs")?;
    to_js(&rt_app::collections::discover(dirs))
}

/// The normalized folder of a new collection inside its root.
#[wasm_bindgen(js_name = collectionsCheckDir)]
pub fn collections_check_dir(dir: &str) -> Result<String, JsValue> {
    rt_app::collections::check_new_dir(dir).map_err(app_error)
}

/// The files of a new collection, relative to its folder.
#[wasm_bindgen(js_name = collectionsScaffold)]
pub fn collections_scaffold(spec: JsValue) -> Result<JsValue, JsValue> {
    let spec: NewCollection = from_js(&spec, "spec")?;
    to_js(&rt_app::collections::scaffold(&spec).map_err(app_error)?)
}

/// Search hits of several collections as one list, best first.
#[wasm_bindgen(js_name = collectionsMergeHits)]
pub fn collections_merge_hits(per_collection: JsValue, limit: usize) -> Result<JsValue, JsValue> {
    let per: Vec<(String, Vec<SearchHit>)> = from_js(&per_collection, "perCollection")?;
    to_js(&rt_app::collections::merge_hits(per, limit))
}
