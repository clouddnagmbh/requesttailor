// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --view <name>` and `--view-expr <expression>` (wave 18, strand G): prints what a
//! saved view (`<request>.views.json`) or an expression picks from the response instead of
//! the whole report. A response is reported as received, so what is printed has the known
//! secret values masked (`***`, see `mask`). The flags are
//! parsed in `lib.rs` into [`ViewArgs`]; [`render`] is the hook `rt send` calls.
//!
//! Output: `json` views print the matches as a JSON list (`[]` for no match); `text` views
//! print one match per line (strings as they are); `table` views
//! print a header line and one line per row, cells separated by a tab. The exit code stays the
//! response's. jq is not available in this build.

use base64::Engine as _;
use rt_app::views::{self, ResponseView, ViewDisplay, ViewLanguage, ViewResult};
use rt_core::send::{ReportBody, SendReport};

use crate::SendArgs;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewArgs {
    /// `--view <name>`: a saved view of the request.
    pub name: Option<String>,
    /// `--view-expr <expression>`: JSONPath, or jq where the build has it.
    pub expression: Option<String>,
}

/// Called by `rt send` on the finished report. `Ok(None)`: print the report as usual.
/// `Ok(Some(text))`: print this instead. `Err(message)`: usage error (exit 2).
pub fn render(args: &SendArgs, report: &SendReport) -> Result<Option<String>, String> {
    let view = match (&args.view.name, &args.view.expression) {
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => return Err("use --view or --view-expr, not both.".into()),
        (Some(name), None) => saved(args, name)?,
        (None, Some(expression)) => ResponseView {
            name: String::new(),
            language: ViewLanguage::Jsonpath,
            expression: expression.clone(),
            display: ViewDisplay::Json,
            columns: Vec::new(),
        },
    };
    let Some(response) = &report.response else {
        let why = report
            .error
            .as_ref()
            .map_or_else(|| "there is no response".to_owned(), |e| format!("{e:?}"));
        return Err(format!("nothing to view: {why}"));
    };
    let body = match &response.body {
        ReportBody::Utf8(text) => text.clone().into_bytes(),
        ReportBody::Base64(text) => base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(|e| format!("the body cannot be read: {e}"))?,
    };
    let result = views::apply_full(&body, &view).map_err(|e| e.to_string())?;
    // A body that is not JSON, or too large, cannot be viewed: say so instead of printing nothing.
    if result.values.is_empty()
        && result.rows.is_empty()
        && let Some(notice) = result
            .notice
            .as_deref()
            .filter(|n| !n.contains("matched nothing"))
    {
        return Err(notice.to_owned());
    }
    Ok(Some(mask(text_of(&result), args, report)))
}

/// The response is reported as received, so a server that echoes a secret would print it:
/// the values this send knows as secret (`--var` values, keychain values of the secret
/// variables the request used) become `***` in what the view prints.
fn mask(mut text: String, args: &SendArgs, report: &SendReport) -> String {
    let mut secrets: Vec<String> = args
        .vars
        .iter()
        .map(|(_, value)| value.clone())
        .filter(|v| !v.is_empty())
        .collect();
    if let (Some(environment), Ok((root, _))) =
        (report.environment.as_deref(), crate::find_root(&args.file))
    {
        let store = rt_app::secrets::keychain::KeychainStore::default();
        let collection = crate::oauth::collection_id(&root);
        for used in report.variables.iter().filter(|v| v.secret) {
            let key = rt_app::secrets::SecretKey {
                collection: collection.clone(),
                environment: environment.to_owned(),
                variable: used.name.clone(),
            };
            if let Ok(Some(value)) = rt_app::secrets::SecretStore::get(&store, &key)
                && !value.is_empty()
            {
                secrets.push(value);
            }
        }
    }
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for secret in secrets {
        text = text.replace(&secret, rt_core::send::MASK);
        // The same value as it reads inside a JSON string.
        if let Ok(quoted) = serde_json::to_string(&secret) {
            let inner = &quoted[1..quoted.len() - 1];
            if inner != secret {
                text = text.replace(inner, rt_core::send::MASK);
            }
        }
    }
    text
}

/// The saved view `name` of the request, from `<request>.views.json` next to the file.
fn saved(args: &SendArgs, name: &str) -> Result<ResponseView, String> {
    let (root, request) = crate::find_root(&args.file)?;
    let file = root.join(views::companion_path(&request));
    let text = std::fs::read_to_string(&file).map_err(|_| {
        format!(
            "{request} has no saved views ({} does not exist).",
            file.display()
        )
    })?;
    let parsed = views::parse_file(&text).map_err(|e| e.to_string())?;
    parsed
        .views
        .iter()
        .find(|v| v.name == name)
        .cloned()
        .ok_or_else(|| {
            let known: Vec<&str> = parsed.views.iter().map(|v| v.name.as_str()).collect();
            format!(
                "{request} has no view \"{name}\" (saved: {}).",
                known.join(", ")
            )
        })
}

fn cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn text_of(result: &ViewResult) -> String {
    let mut out = String::new();
    match result.display {
        ViewDisplay::Json => {
            // Always a list, like the node list of a JSONPath query and the app's filter.
            let json = serde_json::to_string_pretty(&result.values);
            out.push_str(&json.unwrap_or_default());
            out.push('\n');
        }
        ViewDisplay::Text => {
            for value in &result.values {
                out.push_str(&cell(value));
                out.push('\n');
            }
        }
        ViewDisplay::Table => {
            if result.columns.is_empty() {
                return out;
            }
            out.push_str(&result.columns.join("\t"));
            out.push('\n');
            for row in &result.rows {
                let cells: Vec<String> = row
                    .iter()
                    .map(|v| cell(v).replace(['\t', '\n'], " "))
                    .collect();
                out.push_str(&cells.join("\t"));
                out.push('\n');
            }
        }
    }
    out
}
