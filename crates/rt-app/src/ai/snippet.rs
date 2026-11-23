// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A request from a pasted snippet. A `curl` command is read by the importer
//! (`rt_format::import`), without a model: it is exact, free and works offline. Any other
//! snippet (code, API documentation text) goes to the model like every other task.

use rt_core::model::{Auth, Item, RequestSpec};
use rt_format::import::{self, ImportFormat};

use super::{AiAnswer, AiError, AiTask};

/// Longest snippet read.
pub const MAX_SNIPPET: usize = 64 * 1024;

/// Whether the snippet is a `curl` command line (after an optional shell prompt `$ `).
pub fn is_curl(snippet: &str) -> bool {
    let text = snippet.trim_start();
    let text = text.strip_prefix("$ ").unwrap_or(text).trim_start();
    match text.strip_prefix("curl") {
        Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
        None => false,
    }
}

fn first_request(items: &[Item]) -> Option<&RequestSpec> {
    items.iter().find_map(|item| match item {
        Item::Request(r) => Some(&r.spec),
        Item::Folder(f) => first_request(&f.items),
    })
}

/// The request a `curl` command describes, with a note on what the importer left out.
pub fn from_curl(snippet: &str) -> Result<AiAnswer, AiError> {
    let bad = |m: String| AiError::Answer(format!("the cURL command could not be read: {m}"));
    let text = snippet.trim_start();
    let text = text.strip_prefix("$ ").unwrap_or(text);
    let imported = import::import(Some(ImportFormat::Curl), "snippet.curl", text.as_bytes())
        .map_err(|e| bad(e.to_string()))?;
    let doc = rt_format::read_collection(&imported.files).map_err(|e| bad(e.to_string()))?;
    let mut spec = first_request(&doc.model().items)
        .ok_or_else(|| bad("it holds no request".to_owned()))?
        .clone();
    // A proposal takes its place in the folder like any new request, and inherits the
    // folder's authentication unless the command carried its own.
    spec.seq = None;
    if spec.auth == Auth::None {
        spec.auth = Auth::Inherit;
    }
    let mut note = String::from("Read from the cURL command, without a model.");
    for loss in &imported.report.losses {
        note.push_str(&format!("\n- {}", loss.detail));
    }
    if imported.report.requests > 1 {
        note.push_str(&format!(
            "\n- the snippet has {} commands; only the first one is used",
            imported.report.requests
        ));
    }
    Ok(AiAnswer {
        text: note,
        request: Some(spec),
        ..Default::default()
    })
}

/// The answer to `task` that needs no model, if there is one: a `curl` snippet. Shells call
/// this before they look at the AI settings, so it works with the assistant not set up.
pub fn local_answer(task: &AiTask) -> Option<Result<AiAnswer, AiError>> {
    match task {
        AiTask::RequestFromSnippet { snippet, .. } if is_curl(snippet) => {
            if snippet.len() > MAX_SNIPPET {
                return Some(Err(AiError::Answer("the snippet is too long".to_owned())));
            }
            Some(from_curl(snippet))
        }
        _ => None,
    }
}
