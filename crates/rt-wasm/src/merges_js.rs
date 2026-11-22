// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand B: the semantic merge for the web shell (`rt_format::merge`, `rt_app::merges`).
//!
//! The web app runs Git itself (isomorphic-git) and calls these for the collection's files,
//! so its pull and its conflict view use the same merge as the desktop app and
//! `rt merge-driver`. Files are `[path, text][]`, as in `scanSecrets`. The web shell's
//! `merges.ts` and `git/repo.ts` declare the TypeScript types themselves.

use std::collections::BTreeMap;

use rt_app::merges::{ConflictChoice, ConflictStages, FieldConflict, FileSet};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{app_error, from_js, to_js};

fn files(value: &JsValue, what: &str) -> Result<FileSet, JsValue> {
    let list: Vec<(String, String)> = from_js(value, what)?;
    Ok(FileSet {
        files: list.into_iter().map(|(p, t)| (p, t.into_bytes())).collect(),
    })
}

fn texts(files: &BTreeMap<String, Vec<u8>>) -> Vec<(String, String)> {
    files
        .iter()
        .map(|(p, b)| (p.clone(), String::from_utf8_lossy(b).into_owned()))
        .collect()
}

#[derive(Serialize)]
struct Merged {
    files: Vec<(String, String)>,
    conflicts: Vec<FieldConflict>,
    text_conflicts: Vec<String>,
    /// Working-tree text of conflicted files (comments at conflicted fields, or Git's
    /// conflict markers for text conflicts).
    marked: Vec<(String, String)>,
}

/// The merge of a pull: the collection's files at the merge base, `HEAD` and the upstream
/// (`rt_app::merges::pull_merge`).
#[wasm_bindgen(js_name = mergeCollections)]
pub fn merge_collections(
    base: JsValue,
    ours: JsValue,
    theirs: JsValue,
) -> Result<JsValue, JsValue> {
    let (b, o, t) = (
        files(&base, "base")?,
        files(&ours, "ours")?,
        files(&theirs, "theirs")?,
    );
    let (result, marked) = rt_app::merges::pull_merge(&b, &o, &t).map_err(app_error)?;
    to_js(&Merged {
        files: texts(&result.files.files),
        conflicts: result.conflicts,
        text_conflicts: result.text_conflicts,
        marked: texts(&marked),
    })
}

/// `MergeState` of the merge in progress: `context` (the collection at `HEAD`) and the index
/// stages of the conflicted files (`ConflictStages[]`).
#[wasm_bindgen(js_name = mergeState)]
pub fn merge_state(context: JsValue, stages: JsValue, merging: bool) -> Result<JsValue, JsValue> {
    let context = files(&context, "context")?;
    let stages: Vec<ConflictStages> = from_js(&stages, "stages")?;
    to_js(&rt_app::merges::state(&context, &stages, merging).map_err(app_error)?)
}

/// What resolving with `choices` (`ConflictChoice[]`) writes: `[path, text | null][]`.
#[wasm_bindgen(js_name = mergeResolution)]
pub fn merge_resolution(
    context: JsValue,
    stages: JsValue,
    choices: JsValue,
) -> Result<JsValue, JsValue> {
    let context = files(&context, "context")?;
    let stages: Vec<ConflictStages> = from_js(&stages, "stages")?;
    let choices: Vec<ConflictChoice> = from_js(&choices, "choices")?;
    let out = rt_app::merges::resolution(&context, &stages, &choices).map_err(app_error)?;
    let out: Vec<(String, Option<String>)> = out
        .into_iter()
        .map(|(p, b)| (p, b.map(|b| String::from_utf8_lossy(&b).into_owned())))
        .collect();
    to_js(&out)
}

#[derive(Serialize)]
struct TextMerged {
    text: String,
    clean: bool,
}

/// The line merge of a file outside the collection (`rt_format::merge::merge_file` without a
/// path): `{ text, clean }`, with Git's conflict markers where it is not clean.
#[wasm_bindgen(js_name = mergeText)]
pub fn merge_text(base: Option<String>, ours: &str, theirs: &str) -> Result<JsValue, JsValue> {
    let out = rt_format::merge::merge_file(
        None,
        base.as_deref().map(str::as_bytes),
        Some(ours.as_bytes()),
        Some(theirs.as_bytes()),
    );
    to_js(&TextMerged {
        text: String::from_utf8_lossy(&out.bytes.unwrap_or_default()).into_owned(),
        clean: out.conflicts.is_empty() && !out.text_conflict,
    })
}
