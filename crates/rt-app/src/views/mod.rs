// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Saved views of a response (wave 18, strand G): a JSONPath (or jq, if this build has it)
//! over the body with a display, kept per request in `<request>.views.json` (version 1,
//! fixed keys and order; it never holds a value from a response). The expression runs in
//! Rust on the `ResponseStore`; the UI gets only the rows. Interface: types only.

use serde::{Deserialize, Serialize};

use crate::workspace::Workspace;
use crate::{AppError, ResponseStore, companions};

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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// A result is cut at this many bytes of JSON (the UI gets rows, never the whole body).
pub const RESULT_BYTES_LIMIT: usize = 2 * 1024 * 1024;
/// A table shows at most this many rows.
pub const TABLE_ROWS_LIMIT: usize = 5_000;
/// A table shows at most this many columns.
pub const TABLE_COLUMNS_LIMIT: usize = 50;
/// Without `columns`, the keys of this many leading rows give the columns.
const COLUMN_SAMPLE_ROWS: usize = 20;
const NAME_LIMIT: usize = 80;
const EXPRESSION_LIMIT: usize = 4096;

const JQ_MISSING: &str = "jq is not available in this build";

/// The languages this build evaluates. jq is reserved in the file format: no pure Rust jq
/// engine offers a step or time limit, so a hostile filter could not be stopped (in the browser
/// there is no thread to abandon).
pub fn languages() -> Vec<ViewLanguage> {
    vec![ViewLanguage::Jsonpath]
}

/// Reads `<request>.views.json`. Unknown keys and other versions are refused, so a file is
/// never rewritten with parts dropped.
pub fn parse_file(text: &str) -> Result<ViewsFile, AppError> {
    let file: ViewsFile =
        serde_json::from_str(text).map_err(|e| AppError::Format(format!("saved views: {e}")))?;
    if file.version != VIEWS_VERSION {
        return Err(AppError::Format(format!(
            "saved views: version {} is not supported (this build reads version {VIEWS_VERSION})",
            file.version
        )));
    }
    Ok(file)
}

/// The file text: two-space indent, fixed key order, final newline.
pub fn render_file(file: &ViewsFile) -> String {
    let mut text = serde_json::to_string_pretty(file).unwrap_or_default();
    text.push('\n');
    text
}

fn read_file(workspace: &Workspace, request: &str) -> Result<ViewsFile, AppError> {
    match companions::read(workspace, &companion_path(request)) {
        Some(text) => parse_file(&text),
        None => Ok(ViewsFile::default()),
    }
}

fn write_file(
    workspace: &mut Workspace,
    request: &str,
    file: &ViewsFile,
) -> Result<Vec<ResponseView>, AppError> {
    companions::stage(workspace, &companion_path(request), &render_file(file))?;
    Ok(file.views.clone())
}

/// The saved views of the request at `path`, unsaved ones included.
pub fn list(workspace: &Workspace, path: &str) -> Result<Vec<ResponseView>, AppError> {
    workspace.request(path)?;
    Ok(read_file(workspace, path)?.views)
}

/// Adds the view or replaces the one of the same name (in place), staged unsaved.
pub fn save(
    workspace: &mut Workspace,
    path: &str,
    mut view: ResponseView,
) -> Result<Vec<ResponseView>, AppError> {
    workspace.request(path)?;
    view.name = view.name.trim().to_owned();
    if view.name.is_empty()
        || view.name.chars().count() > NAME_LIMIT
        || view.name.chars().any(char::is_control)
    {
        return Err(AppError::Filter(format!(
            "a view needs a name of 1 to {NAME_LIMIT} characters"
        )));
    }
    if view.expression.trim().is_empty() || view.expression.len() > EXPRESSION_LIMIT {
        return Err(AppError::Filter("a view needs an expression".into()));
    }
    check_expression(&view)?;
    if view.display == ViewDisplay::Table {
        view.columns.retain(|c| !c.is_empty());
        view.columns.truncate(TABLE_COLUMNS_LIMIT);
    } else {
        view.columns.clear();
    }
    let mut file = read_file(workspace, path)?;
    match file.views.iter_mut().find(|v| v.name == view.name) {
        Some(slot) => *slot = view,
        None => file.views.push(view),
    }
    write_file(workspace, path, &file)
}

/// Removes the view of that name, staged unsaved.
pub fn remove(
    workspace: &mut Workspace,
    path: &str,
    name: &str,
) -> Result<Vec<ResponseView>, AppError> {
    workspace.request(path)?;
    let mut file = read_file(workspace, path)?;
    file.views.retain(|v| v.name != name);
    write_file(workspace, path, &file)
}

fn check_expression(view: &ResponseView) -> Result<(), AppError> {
    match view.language {
        ViewLanguage::Jq => Err(AppError::Unsupported(JQ_MISSING.into())),
        ViewLanguage::Jsonpath => serde_json_path::JsonPath::parse(&view.expression)
            .map(|_| ())
            .map_err(|e| AppError::Filter(format!("invalid JSONPath expression: {e}"))),
    }
}

/// Evaluates `view` on the kept response `id`, on the Rust side; only the shaped result goes
/// to the caller.
pub fn apply(
    responses: &ResponseStore,
    id: u64,
    view: &ResponseView,
) -> Result<ViewResult, AppError> {
    apply_body(responses.body(id)?, view)
}

fn notice(display: ViewDisplay, text: impl Into<String>) -> ViewResult {
    ViewResult {
        display,
        values: Vec::new(),
        columns: Vec::new(),
        rows: Vec::new(),
        notice: Some(text.into()),
    }
}

/// Evaluates `view` on a body, with the cuts of the UI (rows and bytes).
pub fn apply_body(body: &[u8], view: &ResponseView) -> Result<ViewResult, AppError> {
    evaluate(body, view, true)
}

/// Evaluates `view` on a body without the cuts of the UI, for `rt send --view`, where the
/// caller asked for the whole result (the body limit still holds).
pub fn apply_full(body: &[u8], view: &ResponseView) -> Result<ViewResult, AppError> {
    evaluate(body, view, false)
}

fn evaluate(body: &[u8], view: &ResponseView, cut: bool) -> Result<ViewResult, AppError> {
    if view.language == ViewLanguage::Jq {
        return Err(AppError::Unsupported(JQ_MISSING.into()));
    }
    let path = serde_json_path::JsonPath::parse(&view.expression)
        .map_err(|e| AppError::Filter(format!("invalid JSONPath expression: {e}")))?;
    if body.len() > VIEW_BODY_LIMIT {
        return Ok(notice(
            view.display,
            format!(
                "The body is larger than 32 MiB ({} MiB); a view is not applied to it.",
                body.len().div_ceil(1024 * 1024)
            ),
        ));
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Ok(notice(view.display, "The body is not JSON."));
    };
    let matches = path.query(&value).all();
    if matches.is_empty() {
        return Ok(notice(view.display, "The expression matched nothing."));
    }
    if view.display == ViewDisplay::Table {
        let items: Vec<&serde_json::Value> = match matches.as_slice() {
            [one] if one.is_array() => one.as_array().map_or_else(Vec::new, |a| a.iter().collect()),
            _ => matches.clone(),
        };
        if items.iter().all(|v| v.is_object()) && !items.is_empty() {
            return Ok(table(&items, &view.columns, cut));
        }
        let mut result = values(ViewDisplay::Json, &matches, cut);
        let why = "A table needs an array of objects; shown as JSON.";
        result.notice = Some(match result.notice.take() {
            Some(more) => format!("{why} {more}"),
            None => why.into(),
        });
        return Ok(result);
    }
    Ok(values(view.display, &matches, cut))
}

/// A rough size of the value as JSON (no allocation), to cut results.
fn weight(v: &serde_json::Value) -> usize {
    use serde_json::Value::*;
    match v {
        Null | Bool(_) => 5,
        Number(_) => 12,
        String(s) => s.len() + 2,
        Array(a) => 2 + a.iter().map(|x| weight(x) + 1).sum::<usize>(),
        Object(o) => {
            2 + o
                .iter()
                .map(|(k, x)| k.len() + 4 + weight(x))
                .sum::<usize>()
        }
    }
}

const CUT: &str = "The result is cut at 2 MiB; narrow the expression.";

fn values(display: ViewDisplay, matches: &[&serde_json::Value], cut: bool) -> ViewResult {
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut was_cut = false;
    for m in matches {
        total += weight(m) + 2;
        if cut && total > RESULT_BYTES_LIMIT {
            was_cut = true;
            break;
        }
        out.push((*m).clone());
    }
    ViewResult {
        display,
        values: out,
        columns: Vec::new(),
        rows: Vec::new(),
        notice: was_cut.then(|| CUT.to_owned()),
    }
}

fn table(items: &[&serde_json::Value], wanted: &[String], cut: bool) -> ViewResult {
    let columns: Vec<String> = if wanted.is_empty() {
        let mut seen: Vec<String> = Vec::new();
        for item in items.iter().take(COLUMN_SAMPLE_ROWS) {
            if let Some(object) = item.as_object() {
                for key in object.keys() {
                    if !seen.contains(key) && seen.len() < TABLE_COLUMNS_LIMIT {
                        seen.push(key.clone());
                    }
                }
            }
        }
        seen
    } else {
        wanted.iter().take(TABLE_COLUMNS_LIMIT).cloned().collect()
    };
    let mut rows = Vec::new();
    let mut total = 0usize;
    let take = if cut { TABLE_ROWS_LIMIT } else { usize::MAX };
    for item in items.iter().take(take) {
        let object = item.as_object();
        let row: Vec<serde_json::Value> = columns
            .iter()
            .map(|c| {
                object
                    .and_then(|o| o.get(c))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            })
            .collect();
        total += row.iter().map(|v| weight(v) + 1).sum::<usize>();
        if cut && total > RESULT_BYTES_LIMIT {
            break;
        }
        rows.push(row);
    }
    let notice = (rows.len() < items.len())
        .then(|| format!("Showing the first {} of {} rows.", rows.len(), items.len()));
    ViewResult {
        display: ViewDisplay::Table,
        values: Vec::new(),
        columns,
        rows,
        notice,
    }
}
