// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The prompts of the app's own tasks, kept in versioned files (`prompts/<id>.md`).
//!
//! A file starts with a header between `---` lines:
//!
//! ```text
//! ---
//! id: suggest-names
//! version: 1
//! answer: json        # json | markdown | none (a fragment)
//! profile: fast       # fast | smart | none (a fragment)
//! ---
//! the text, with <<name>> for a value of the task and <<include:id>> for a fragment
//! ```
//!
//! A change of a prompt raises `version` and needs recorded answers for the new version
//! (`testdata/ai/fixtures/tasks/<id>@<version>/`); a version without fixtures fails the
//! tests. Values of the task are inserted in one pass, so a description that contains
//! `<<context>>` stays text.

use super::{AiNameTarget, AiTask};

/// One parsed prompt file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prompt {
    pub id: &'static str,
    pub version: u32,
    /// `json`, `markdown` or `none` (fragments and the shared system prompt's own shape).
    pub answer: &'static str,
    /// `fast`, `smart` or `none`.
    pub profile: &'static str,
    pub body: &'static str,
}

/// Every prompt file, by id.
pub const SOURCES: &[(&str, &str)] = &[
    ("system", include_str!("prompts/system.md")),
    ("request-shape", include_str!("prompts/request-shape.md")),
    ("assertions-api", include_str!("prompts/assertions-api.md")),
    ("create-request", include_str!("prompts/create-request.md")),
    ("explain-error", include_str!("prompts/explain-error.md")),
    ("suggest-tests", include_str!("prompts/suggest-tests.md")),
    (
        "request-from-snippet",
        include_str!("prompts/request-from-snippet.md"),
    ),
    (
        "explain-response",
        include_str!("prompts/explain-response.md"),
    ),
    (
        "suggest-assertions",
        include_str!("prompts/suggest-assertions.md"),
    ),
    ("suggest-names", include_str!("prompts/suggest-names.md")),
    ("draft-docs", include_str!("prompts/draft-docs.md")),
];

fn parse(source: &'static str) -> Option<Prompt> {
    let rest = source.strip_prefix("---\n")?;
    let (head, body) = rest.split_once("\n---\n")?;
    let mut prompt = Prompt {
        id: "",
        version: 0,
        answer: "",
        profile: "",
        body: body.strip_suffix('\n').unwrap_or(body),
    };
    for line in head.lines() {
        let (key, value) = line.split_once(':')?;
        let value = value.split('#').next().unwrap_or("").trim();
        match key.trim() {
            "id" => prompt.id = value,
            "version" => prompt.version = value.parse().ok()?,
            "answer" => prompt.answer = value,
            "profile" => prompt.profile = value,
            _ => return None,
        }
    }
    (!prompt.id.is_empty() && prompt.version > 0 && !prompt.answer.is_empty()).then_some(prompt)
}

/// All prompts; `None` entries cannot happen in a build that passes the tests.
pub fn all() -> Vec<Prompt> {
    SOURCES.iter().filter_map(|(_, s)| parse(s)).collect()
}

/// The prompt `id`.
pub fn get(id: &str) -> Option<Prompt> {
    SOURCES
        .iter()
        .find(|(i, _)| *i == id)
        .and_then(|(_, s)| parse(s))
}

/// The prompt `id` with `values` inserted for `<<name>>` and fragments for `<<include:id>>`.
/// Unknown names stay as written.
pub fn render(id: &str, values: &[(&str, &str)]) -> String {
    let Some(prompt) = get(id) else {
        return String::new();
    };
    expand(prompt.body, values, 0)
}

fn expand(template: &str, values: &[(&str, &str)], depth: usize) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("<<") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find(">>") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        let known = if let Some(fragment) = name.strip_prefix("include:") {
            // Fragments hold no values and no further fragments.
            get(fragment)
                .filter(|_| depth == 0)
                .map(|p| expand(p.body, &[], depth + 1))
        } else {
            values
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (*v).to_owned())
        };
        match known {
            Some(text) => {
                out.push_str(&text);
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("<<");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

impl AiTask {
    /// The id of the task's prompt file.
    pub fn prompt_id(&self) -> &'static str {
        match self {
            AiTask::CreateRequest { .. } => "create-request",
            AiTask::ExplainError { .. } => "explain-error",
            AiTask::SuggestTests { .. } => "suggest-tests",
            AiTask::RequestFromSnippet { .. } => "request-from-snippet",
            AiTask::ExplainResponse { .. } => "explain-response",
            AiTask::SuggestAssertions { .. } => "suggest-assertions",
            AiTask::SuggestNames { .. } => "suggest-names",
            AiTask::DraftDocs { .. } => "draft-docs",
        }
    }

    /// The version of the task's prompt (the fixtures of that version are its tests).
    pub fn prompt_version(&self) -> u32 {
        get(self.prompt_id()).map_or(0, |p| p.version)
    }
}

impl AiNameTarget {
    /// The sentence that tells the model what to name.
    pub(crate) fn ask(self) -> &'static str {
        match self {
            AiNameTarget::Request => {
                "propose names for this request: short (two to five words), saying what it does, with no HTTP method and no URL in them."
            }
            AiNameTarget::Folder => {
                "propose names for this folder: one to three words naming the resource or area its requests belong to."
            }
            AiNameTarget::Variable => {
                "propose names for the variables this request should use instead of the literal values in it (host, ids, tokens, repeated values). Give camelCase names made of letters, digits and underscores, one per value worth a variable."
            }
        }
    }
}
