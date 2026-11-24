// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Saved views of a response (wave 18, strand G): a JSONPath (or jq, if this build has it)
//! over the body with a display, kept per request in `<request>.views.json` (version 1,
//! fixed keys and order; it never holds a value from a response). The expression runs in
//! Rust on the `ResponseStore`; the UI gets only the rows. Interface: types only.

use serde::{Deserialize, Serialize};

pub const VIEWS_VERSION: u32 = 1;
/// Bodies larger than this (32 MiB) get a hint instead of a view.
pub const VIEW_BODY_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViewLanguage {
    Jsonpath,
    /// Only where a build has it (`ViewsApi.languages`).
    Jq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViewDisplay {
    Json,
    /// An array of objects as a table.
    Table,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseView {
    pub name: String,
    pub language: ViewLanguage,
    pub expression: String,
    pub display: ViewDisplay,
    /// Columns of a table (keys of the objects); empty: all keys of the first rows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
}

/// The contents of `<request>.views.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewsFile {
    pub version: u32,
    pub views: Vec<ResponseView>,
}

impl Default for ViewsFile {
    fn default() -> Self {
        Self {
            version: VIEWS_VERSION,
            views: Vec::new(),
        }
    }
}

/// The companion file of a request (`users/get.yml` → `users/get.views.json`).
pub fn companion_path(request: &str) -> String {
    let stem = request.rsplit_once('.').map_or(request, |(stem, _)| stem);
    format!("{stem}.views.json")
}

/// What a view shows for one response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewResult {
    pub display: ViewDisplay,
    /// The values for `json` and `text`: one entry per match.
    pub values: Vec<serde_json::Value>,
    /// Table: the column names and the rows (cells are JSON values).
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Vec<serde_json::Value>>,
    /// The body is not JSON, the expression did not match, or the body is over the limit.
    #[serde(default)]
    pub notice: Option<String>,
}
