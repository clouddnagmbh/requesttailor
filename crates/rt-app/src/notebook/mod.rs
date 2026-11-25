// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! API notebooks (wave 19, strand W): `Backend.notebook`, `rt notebook run`. A notebook is a
//! Markdown companion file `<name>.notebook.md` in the collection (files are the truth;
//! OpenCollection is not touched, the reader only reads `.yml`): prose, executable request
//! blocks, variables passed between blocks. **Outputs are never saved** (a response, a table
//! from a JSONPath, a simple chart are shown in the view and held in memory), only on request
//! as a snapshot (the existing snapshot feature, not this module).
//!
//! Format: ordinary Markdown. A request block is a fenced code block whose info string is
//! `rt-request`; everything outside such blocks is prose and is kept byte for byte. The body
//! of a block is a small, YAML-like list of `key: value` lines (two-space indents):
//!
//! ```text
//! id: login            optional, unique; a block without one is `block-<n>`
//! ref: auth/login.yml  a request file of the collection ...
//! method: POST         ... or an embedded request: method (default GET), url, headers:
//! url: {{base}}/login  (indented `Name: value` lines) and body (`body: <text>` or `body: |`
//! headers:             and indented lines; raw bodies only)
//!   Accept: application/json
//! set:                 variables taken from the response: `name: $.json.path` (the first match)
//!   token: $.access_token
//! show:                what the view shows of the response (never stored)
//!   - response
//!   - table: $.items[*]
//!   - chart: $.values
//! ```
//!
//! `{{variables}}` resolve like in any request; earlier blocks' `set:` values are runtime
//! variables for the following ones. Secrets are `{{secret:name}}` (or secret variables of the
//! environment) and never appear in the file or an output; a variable set from a response whose
//! name looks like a credential (`token`, `secret`, `password`, `key`, `auth`) is carried to the
//! next block but shown as `***`.
//!
//! Running a block sends a request: the send guard, the production question (asked by the
//! shell), the rate limits of the host and the audit log work as for any send. A writing
//! block runs only if the user allowed writes for this run, and never against a production
//! environment (`skipped (writes)`, `skipped (production)`). One audit entry per run.

use serde::{Deserialize, Serialize};

use rt_core::model::RequestSpec;

mod format;
mod run;

pub use format::{FENCE_INFO, default_id, parse, render};
pub use run::{NotebookEnv, SKIPPED_EARLIER, SKIPPED_PRODUCTION, SKIPPED_WRITES, run, run_with};

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
    /// Variables of earlier blocks the view holds (a single block runs with them); a masked
    /// value is not carried. Running all blocks needs none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vars: Vec<(String, String)>,
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
    /// What the `show:` lines of the block make of the response (tables and chart values;
    /// `response` is the report itself). Held in memory by the view, never saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<BlockView>,
    /// Problems worth a line in the view (a `set:` path that matched nothing).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// One `show:` item shaped for the view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockView {
    pub show: Show,
    /// Table rows or the matched values; `notice` says why there are none.
    pub result: crate::views::ViewResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotebookOutcome {
    pub results: Vec<BlockResult>,
}

/// Whether `path` names a notebook file.
pub fn is_notebook(path: &str) -> bool {
    path.ends_with(SUFFIX)
}

fn stem_of(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(SUFFIX).unwrap_or(file)
}

/// Reads `path` with the file name as the title when it has no heading.
pub fn parse_named(path: &str, text: &str) -> Result<Notebook, crate::AppError> {
    let mut notebook = parse(text)?;
    if notebook.title.is_empty() {
        notebook.title = stem_of(path).to_owned();
    }
    Ok(notebook)
}

/// The notebook files of the collection (unsaved ones included), sorted by path. A file that
/// does not parse is listed with its problem as the title and no blocks, so it can be fixed.
pub fn list(workspace: &crate::workspace::Workspace) -> Vec<NotebookEntry> {
    crate::companions::list(workspace, "")
        .into_iter()
        .filter(|p| is_notebook(p))
        .map(|path| {
            let text = crate::companions::read(workspace, &path).unwrap_or_default();
            match parse_named(&path, &text) {
                Ok(n) => NotebookEntry {
                    blocks: n
                        .blocks
                        .iter()
                        .filter(|b| matches!(b, Block::Request { .. }))
                        .count() as u32,
                    title: n.title,
                    path,
                },
                Err(e) => NotebookEntry {
                    title: format!("{} (unreadable: {e})", stem_of(&path)),
                    blocks: 0,
                    path,
                },
            }
        })
        .collect()
}

/// Reads a notebook of the collection.
pub fn read(
    workspace: &crate::workspace::Workspace,
    path: &str,
) -> Result<Notebook, crate::AppError> {
    if !is_notebook(path) {
        return Err(crate::AppError::Shell(format!(
            "{path} is not a notebook file (it ends in {SUFFIX})"
        )));
    }
    let text = crate::companions::read(workspace, path)
        .ok_or_else(|| crate::AppError::Shell(format!("no notebook {path}")))?;
    parse_named(path, &text)
}

/// Stages `notebook` as the file `path` for the next save (unsaved until then).
pub fn stage(
    workspace: &mut crate::workspace::Workspace,
    path: &str,
    notebook: &Notebook,
) -> Result<(), crate::AppError> {
    if !is_notebook(path) {
        return Err(crate::AppError::Shell(format!(
            "{path} is not a notebook file (it ends in {SUFFIX})"
        )));
    }
    let text = render(notebook)?;
    crate::companions::stage(workspace, path, &text)
}
