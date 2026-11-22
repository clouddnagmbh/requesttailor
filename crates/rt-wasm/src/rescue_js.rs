// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 12, strand G: secret rescue: history scan helpers and the value replacement (`rt_app::rescue`).
//!
//! The web shell walks the repository with isomorphic-git (`apps/web/src/git/rewrite.ts`)
//! and leaves everything that decides what is found and how it is replaced to the core, so
//! the browser finds and writes exactly what the desktop app does:
//!
//! - `HistoryScanner`: `rt_app::rescue::HistoryScanner` (blobs and commits in, findings out).
//! - `rescueFindingRefs(plan)`: checks a plan and says which files hold its findings.
//! - `RescueRewriter`: built from those files' texts, it keeps the value inside the core and
//!   rewrites texts (`rt_app::rescue::replace_value`); `value()` is for the secret store only.
//!
//! The shell's file declares these TypeScript types itself (`apps/web/src/git/rewrite.ts`), so
//! `apps/web/src/wasm.ts` stays untouched.

use std::collections::HashMap;

use rt_app::git::GitCommit;
use rt_app::rescue::{self, FindingRef, RescuePlan};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{app_error, from_js, to_js};

#[wasm_bindgen(js_name = HistoryScanner)]
pub struct HistoryScannerJs(rescue::HistoryScanner);

impl Default for HistoryScannerJs {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_class = HistoryScanner)]
impl HistoryScannerJs {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(rescue::HistoryScanner::new())
    }

    /// Whether blob `id` was added already.
    pub fn knows(&self, id: &str) -> bool {
        self.0.knows(id)
    }

    /// A blob's text (`undefined` for files the scanner skips), first seen at `path`.
    #[wasm_bindgen(js_name = addBlob)]
    pub fn add_blob(&mut self, id: &str, path: &str, text: Option<String>) {
        self.0.add_blob(id, path, text.as_deref());
    }

    /// A commit (`GitCommit`), its parents and its collection files (`[path, blob][]`).
    #[wasm_bindgen(js_name = addCommit)]
    pub fn add_commit(
        &mut self,
        commit: JsValue,
        parents: Vec<String>,
        files: JsValue,
    ) -> Result<(), JsValue> {
        let commit: GitCommit = from_js(&commit, "commit")?;
        let files: Vec<(String, String)> = from_js(&files, "files")?;
        self.0.add_commit(commit, parents, files);
        Ok(())
    }

    /// `HistoryScan` of what was added: `branches` are `[name, tip][]`.
    pub fn finish(&self, branches: JsValue, head: Option<String>) -> Result<JsValue, JsValue> {
        let branches: Vec<(String, String)> = from_js(&branches, "branches")?;
        to_js(&self.0.finish(&branches, head.as_deref()))
    }
}

#[derive(Serialize)]
struct FileAt {
    commit: String,
    path: String,
}

/// The files (`{commit, path}[]`, collection-relative) that hold `plan`'s findings; throws
/// for an invalid plan (no finding, bad variable name, no environment).
#[wasm_bindgen(js_name = rescueFindingRefs)]
pub fn rescue_finding_refs(plan: JsValue) -> Result<JsValue, JsValue> {
    let plan: RescuePlan = from_js(&plan, "plan")?;
    let refs = rescue::check_plan(&plan).map_err(app_error)?;
    to_js(
        &refs
            .into_iter()
            .map(|r| FileAt {
                commit: r.commit,
                path: r.path,
            })
            .collect::<Vec<_>>(),
    )
}

/// The rewrite of one value. The value stays in here: the shell only hands texts in and gets
/// texts back.
#[wasm_bindgen(js_name = RescueRewriter)]
pub struct RescueRewriterJs {
    value: String,
    variable: String,
}

#[wasm_bindgen(js_class = RescueRewriter)]
impl RescueRewriterJs {
    /// From `plan` and the texts of its files (`[commit, path, text | null][]`, as
    /// `rescueFindingRefs` named them). Throws when a finding is not there, the findings are
    /// about different values, or the value cannot be replaced safely.
    #[wasm_bindgen(constructor)]
    pub fn new(plan: JsValue, texts: JsValue) -> Result<RescueRewriterJs, JsValue> {
        let plan: RescuePlan = from_js(&plan, "plan")?;
        let texts: Vec<(String, String, Option<String>)> = from_js(&texts, "texts")?;
        let texts: HashMap<(String, String), Option<String>> = texts
            .into_iter()
            .map(|(c, p, t)| ((c.to_ascii_lowercase(), p), t))
            .collect();
        let read = |at: &FindingRef| {
            Ok(texts
                .get(&(at.commit.clone(), at.path.clone()))
                .cloned()
                .flatten())
        };
        let value = rescue::plan_value(&plan, read).map_err(app_error)?;
        Ok(Self {
            value,
            variable: plan.variable,
        })
    }

    /// `text` rewritten, or `undefined` when it does not hold the value.
    pub fn rewrite(&self, text: &str) -> Option<String> {
        rescue::contains_value(text, &self.value)
            .then(|| rescue::replace_value(text, &self.value, &self.variable))
    }

    /// The value, for the secret store (the shell's encrypted browser storage) only.
    pub fn value(&self) -> String {
        self.value.clone()
    }
}
