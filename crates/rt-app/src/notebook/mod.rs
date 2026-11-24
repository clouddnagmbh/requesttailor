// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! API notebooks (wave 19, strand W): `Backend.notebook`, `rt notebook run`. A notebook is a
//! Markdown companion file `<name>.notebook.md` in the collection (files are the truth;
//! OpenCollection is not touched, the reader only reads `.yml`): prose, executable request
//! blocks, variables passed between blocks. **Outputs are never saved** (a response, a table
//! from a JSONPath, a simple chart are shown in the view), only on request as a snapshot.
//!
//! Format: ordinary Markdown. A request block is a fenced code block whose info string is
//! `rt-request`; its body is YAML with `ref: <path of a request file>` or an embedded
//! request (`method`, `url`, `headers`, `body`) and an optional `id`, `set:` (variables to
//! take from the response: `name: $.json.path`) and `show:` (`response`, `table: $.items[*]`,
//! `chart: $.values`). `{{variables}}` resolve like in any request; earlier blocks' `set:`
//! values are runtime variables for the following ones. Secrets are `{{secret:name}}` and
//! never appear in the file or an output.
//!
//! Running a block sends a request: guard, production question, rate tracker and audit as
//! usual; `rt notebook run` sends reading blocks and writing ones only with `--allow-writes`,
//! never against production. Interface: types and the two hooks [`parse`] and [`render`]
//! (answer "not implemented yet").

use serde::{Deserialize, Serialize};

use rt_core::model::RequestSpec;

/// A notebook file's suffix.
pub const SUFFIX: &str = ".notebook.md";
/// Most blocks run in one go.
pub const MAX_BLOCKS: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[allow(clippy::large_enum_variant)] // a notebook holds a few blocks; boxing would change the JSON shape of nothing but the code
pub enum Block {
    /// Markdown prose, kept as written.
    Text { markdown: String },
    /// An executable request.
    Request {
        /// Stable within the notebook (a block without one gets `block-<n>`).
        id: String,
        /// A request file of the collection, or `None` for an embedded request.
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "ref")]
        request_ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        embedded: Option<RequestSpec>,
        /// Variables taken from the response: name → JSONPath.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        set: Vec<(String, String)>,
        /// What the view shows of the response.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        show: Vec<Show>,
    },
}

/// A view of a response; never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Show {
    Response,
    Table { path: String },
    Chart { path: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notebook {
    /// The first `# heading`, or the file name.
    pub title: String,
    pub blocks: Vec<Block>,
}

/// A notebook file of the active collection (`NotebookApi.list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookEntry {
    /// Path relative to the collection, ending in [`SUFFIX`].
    pub path: String,
    pub title: String,
    pub blocks: u32,
}

/// Run one block, or all (`NotebookApi.run`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookRun {
    pub path: String,
    /// `None`: every request block in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// Writing blocks run only if the user allowed it for this run (never against
    /// production).
    #[serde(default)]
    pub allow_writes: bool,
}

/// What running a block produced (kept in memory by the view, not saved).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockResult {
    pub block: String,
    /// The masked report of the send; `None` when the block was skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<rt_core::send::SendReport>,
    /// Variables set from the response (secret ones masked).
    #[serde(default)]
    pub set: Vec<(String, String)>,
    /// Why it was skipped (`skipped (writes)`, `skipped (production)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotebookOutcome {
    pub results: Vec<BlockResult>,
}

/// Reads a notebook file's text. Interface: answers "not implemented yet".
pub fn parse(text: &str) -> Result<Notebook, crate::AppError> {
    let _ = text;
    Err(crate::AppError::Unsupported(
        "Notebooks are not implemented yet (wave 19, strand W).".into(),
    ))
}

/// Writes a notebook back; reading and writing again leaves an unchanged file byte for byte.
/// Interface: answers "not implemented yet".
pub fn render(notebook: &Notebook) -> Result<String, crate::AppError> {
    let _ = notebook;
    Err(crate::AppError::Unsupported(
        "Notebooks are not implemented yet (wave 19, strand W).".into(),
    ))
}
