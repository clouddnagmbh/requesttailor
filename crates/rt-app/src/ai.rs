// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The AI assistant (wave 8): a request from a description, an error explained, tests
//! suggested. Off until the user configures a provider; no provider is built in or preferred,
//! and nothing goes to CloudDNA.
//!
//! - Providers ([`AiProvider`]): `anthropic` (Messages API, the user's own key) and
//!   `openai-compatible` (any `/v1/chat/completions` endpoint: a local model such as Ollama or
//!   LM Studio, or a hosted one). Both go through the app's own
//!   [`Transport`](rt_core::transport::Transport), so the web app calls them with `fetch` or
//!   the companion like any request. Keys live in the OS keychain (web: encrypted browser
//!   storage), never in collection files.
//! - Masking before every call ([`redact`]): the context sent to a model is built from masked
//!   reports and requests, and the whole outgoing payload is masked again with every secret
//!   value in scope. A test proves no secret value reaches a provider (gate).
//! - Answers are proposals: a request or tests are shown for confirmation and created
//!   unsaved, like plugin additions.
//!
//! Owner: wave 8 interface. Implementation: strand E.

use rt_core::model::{Assertion, RequestSpec, Script};
use rt_core::services::LocalBoxFuture;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProviderKind {
    Anthropic,
    OpenaiCompatible,
}

/// The user's configuration, stored in the app's settings (not in the collection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: AiProviderKind,
    /// e.g. `claude-opus-5-5` for Anthropic, `llama3.1` for a local Ollama.
    pub model: String,
    /// `None`: the provider's public endpoint. For `openai-compatible` required, e.g.
    /// `http://localhost:11434/v1`.
    pub base_url: Option<String>,
}

/// What the user asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "kebab-case")]
pub enum AiTask {
    /// "POST a new user with name and email to the staging API".
    CreateRequest {
        description: String,
        /// Folder the request will go in; its sibling requests are context (masked).
        folder: String,
    },
    /// Why did this fail? Uses the masked report of the response.
    ExplainError { path: String, response: u64 },
    /// Tests (a `tests` script) and assertions for the request and its response.
    SuggestTests { path: String, response: u64 },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AiAnswer {
    /// Explanation for the user (Markdown).
    pub text: String,
    /// `CreateRequest`: the proposed request (unsaved until the user confirms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestSpec>,
    /// `SuggestTests`: proposed scripts and assertions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<Script>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: AiRole,
    pub content: String,
}

/// One model call, provider-neutral. Already masked when it reaches a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiCall {
    pub system: String,
    pub messages: Vec<AiMessage>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum AiError {
    #[error("no AI provider is set up")]
    NotConfigured,
    #[error("no API key for this provider")]
    NoKey,
    /// HTTP or transport failure, masked.
    #[error("the provider could not be reached: {0}")]
    Transport(String),
    /// The provider answered with an error (status and its message, masked).
    #[error("the provider refused: {0}")]
    Provider(String),
    /// The model's answer could not be used (e.g. no valid request in it).
    #[error("the answer could not be used: {0}")]
    Answer(String),
}

/// A model behind an API. Not `Send` (wasm32).
pub trait AiProvider {
    fn complete<'a>(&'a self, call: AiCall) -> LocalBoxFuture<'a, Result<String, AiError>>;
}

/// Replaces every occurrence of each non-empty secret value with `***` (longest first), the
/// same rule as report masking. Applied to the whole serialized payload before it leaves.
pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut values: Vec<&str> = secrets
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    values.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = text.to_owned();
    for value in values {
        out = out.replace(value, "***");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn longest_secret_first_and_empty_ignored() {
        let secrets = vec!["ab".to_owned(), "abc".to_owned(), String::new()];
        assert_eq!(redact("x abc ab y", &secrets), "x *** *** y");
    }
}
