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
//!
//! # How a task runs
//!
//! 1. The shell builds an [`AiContext`] from the open [`Workspace`](crate::Workspace) and the
//!    [`ResponseStore`](crate::ResponseStore): the request spec (credential values that are
//!    not `{{variables}}` replaced by `***`), the masked report of the response (credential
//!    headers and JSON fields masked, body cut), variable names (never values of secrets)
//!    and, for a new request, the sibling requests of the folder.
//! 2. It collects every secret value in scope ([`secrets_in_scope`], plus OAuth 2.0 tokens,
//!    [`token_values`]) and calls [`run`] with a provider ([`provider`]).
//! 3. [`run`] builds the prompt ([`AiCall`]), masks every string in it and the whole
//!    serialized payload with [`redact`] (raw, escaped, percent-encoded and base64 forms of
//!    every secret), sends it and parses the answer ([`AiAnswer`]). Errors are masked too.
//!
//! Keys: one per provider kind, in the shell's secret store under [`key_slot`] (desktop: the
//! OS keychain, service `RequestTailor`; web: the encrypted browser store). The
//! configuration ([`AiConfig`]) is an app setting (desktop: `ai.json` in the data folder;
//! web: `localStorage`).

use rt_core::model::{Assertion, Collection, Item, RequestSpec, Script, Variable};
use rt_core::services::LocalBoxFuture;
use serde::{Deserialize, Serialize};

use crate::{AppError, SecretKey, SendInput};

mod answer;
mod context;
mod mask;
mod prompt;
mod providers;

pub use context::AiContext;
pub use providers::{
    ANTHROPIC_BASE_URL, ANTHROPIC_VERSION, AnthropicProvider, OpenAiCompatibleProvider, provider,
};

/// The model suggested when the user picks `anthropic` (they can change it).
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";
/// [`SecretKey::collection`] of the provider keys (not a collection: app-wide).
pub const KEY_COLLECTION: &str = "requesttailor:ai";
/// [`SecretKey::environment`] of the provider keys.
pub const KEY_ENVIRONMENT: &str = "keys";

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
///
/// Besides the raw value it replaces the forms a value takes in text that reaches a model:
/// JSON- and Debug-escaped, percent-encoded (`%20` and `+` for spaces), and base64 (standard
/// and URL-safe, with and without padding; values of 6 characters or more).
pub fn redact(text: &str, secrets: &[String]) -> String {
    mask::redact(text, secrets)
}

impl AiProviderKind {
    /// The serialized name, e.g. `openai-compatible`; also the key's
    /// [`SecretKey::variable`].
    pub fn id(self) -> &'static str {
        match self {
            AiProviderKind::Anthropic => "anthropic",
            AiProviderKind::OpenaiCompatible => "openai-compatible",
        }
    }

    /// Whether calls need a key (`openai-compatible`: optional, local models need none).
    pub fn needs_key(self) -> bool {
        matches!(self, AiProviderKind::Anthropic)
    }
}

impl AiConfig {
    /// Checks what the settings dialog lets through: a model name, and a base URL with
    /// `http://` or `https://` (required for `openai-compatible`).
    pub fn validate(&self) -> Result<(), AiError> {
        if self.model.trim().is_empty() {
            return Err(AiError::NotConfigured);
        }
        let url = self.base_url.as_deref().map(str::trim).unwrap_or("");
        if url.is_empty() {
            return if self.provider == AiProviderKind::OpenaiCompatible {
                Err(AiError::NotConfigured)
            } else {
                Ok(())
            };
        }
        if url.starts_with("http://") || url.starts_with("https://") {
            Ok(())
        } else {
            Err(AiError::Transport(format!(
                "the base URL must start with http:// or https://: {url}"
            )))
        }
    }
}

/// Where the key of `provider` is kept in the shell's secret store.
pub fn key_slot(provider: AiProviderKind) -> SecretKey {
    SecretKey {
        collection: KEY_COLLECTION.to_owned(),
        environment: KEY_ENVIRONMENT.to_owned(),
        variable: provider.id().to_owned(),
    }
}

/// Every secret value a send of `input` could put into a request, a response or a report:
///
/// - the secret values the shell passes (keychain or browser store, external secrets),
/// - runtime variables (`--var`, session variables set by scripts) whose name is declared
///   secret anywhere in the collection, whose name names a credential (`token`, `apiKey`,
///   `password`, …), or whose value is long (16 characters or more, not a URL),
/// - values of variables declared secret in a file (they should have none).
///
/// OAuth 2.0 tokens are not in the collection; shells add them with [`token_values`].
pub fn secrets_in_scope(collection: &Collection, input: &SendInput) -> Vec<String> {
    fn declared<'a>(items: &'a [Item], out: &mut Vec<&'a Variable>) {
        for item in items {
            match item {
                Item::Folder(f) => {
                    out.extend(&f.defaults.vars);
                    declared(&f.items, out);
                }
                Item::Request(r) => out.extend(&r.spec.vars),
            }
        }
    }
    let mut vars: Vec<&Variable> = collection.defaults.vars.iter().collect();
    for env in &collection.environments {
        vars.extend(&env.variables);
    }
    declared(&collection.items, &mut vars);
    let secret_names: Vec<&str> = vars
        .iter()
        .filter(|v| v.secret)
        .map(|v| v.name.as_str())
        .collect();
    let mut out: Vec<String> = input.secrets.iter().map(|(_, v)| v.clone()).collect();
    out.extend(
        vars.iter()
            .filter(|v| v.secret)
            .filter_map(|v| v.value.clone()),
    );
    for (name, value) in &input.vars {
        let long = value.chars().count() >= 16
            && !value.starts_with("http://")
            && !value.starts_with("https://");
        if secret_names.contains(&name.as_str()) || mask::credential_name(name) || long {
            out.push(value.clone());
        }
    }
    out.retain(|v| !v.is_empty());
    out.sort();
    out.dedup();
    out
}

/// The values of a collection's stored OAuth 2.0 tokens (access, refresh and ID tokens).
pub fn token_values(tokens: &crate::auth::SecretTokens) -> Result<Vec<String>, AppError> {
    let mut out = Vec::new();
    for info in tokens.list()? {
        if let Some(token) = tokens.get_token(&info.key)? {
            out.push(token.access_token);
            out.extend(token.refresh_token);
            out.extend(token.id_token);
        }
    }
    out.retain(|v| !v.is_empty());
    Ok(out)
}

/// The model call for `task`, built from `context` and masked with `secrets`: every string
/// of the call, then the whole serialized payload. What [`run`] sends.
pub fn prepare(task: &AiTask, context: &AiContext, secrets: &[String]) -> Result<AiCall, AiError> {
    let call = prompt::build(task, context);
    let call = AiCall {
        system: redact(&call.system, secrets),
        messages: call
            .messages
            .into_iter()
            .map(|m| AiMessage {
                role: m.role,
                content: redact(&m.content, secrets),
            })
            .collect(),
        max_tokens: call.max_tokens,
    };
    let json = serde_json::to_string(&call).map_err(|e| AiError::Transport(e.to_string()))?;
    let masked = redact(&json, secrets);
    if masked == json {
        return Ok(call);
    }
    // Something was still found in the serialized form (an escape the string pass did not
    // see). If the masked text is no longer JSON, nothing is sent.
    serde_json::from_str(&masked).map_err(|_| {
        AiError::Transport("the request to the model could not be masked; nothing was sent".into())
    })
}

/// Runs one task: [`prepare`], send through `provider`, parse the answer. Errors are masked
/// with `secrets`. The answer is a proposal; nothing is saved here.
pub async fn run<P: AiProvider + ?Sized>(
    task: &AiTask,
    context: &AiContext,
    provider: &P,
    secrets: &[String],
) -> Result<AiAnswer, AiError> {
    let call = prepare(task, context, secrets)?;
    let text = provider
        .complete(call)
        .await
        .map_err(|e| mask_error(e, secrets))?;
    answer::parse(task, &text).map_err(|e| mask_error(e, secrets))
}

/// `e` with every secret in its message replaced by `***`.
pub fn mask_error(e: AiError, secrets: &[String]) -> AiError {
    let m = |s: String| redact(&s, secrets);
    match e {
        AiError::Transport(s) => AiError::Transport(m(s)),
        AiError::Provider(s) => AiError::Provider(m(s)),
        AiError::Answer(s) => AiError::Answer(m(s)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_secret_first_and_empty_ignored() {
        let secrets = vec!["ab".to_owned(), "abc".to_owned(), String::new()];
        assert_eq!(redact("x abc ab y", &secrets), "x *** *** y");
    }

    #[test]
    fn config_validation() {
        let mut c = AiConfig {
            provider: AiProviderKind::Anthropic,
            model: DEFAULT_ANTHROPIC_MODEL.into(),
            base_url: None,
        };
        assert_eq!(c.validate(), Ok(()));
        c.model = " ".into();
        assert_eq!(c.validate(), Err(AiError::NotConfigured));
        c.model = "llama3.1".into();
        c.provider = AiProviderKind::OpenaiCompatible;
        assert_eq!(c.validate(), Err(AiError::NotConfigured));
        c.base_url = Some("localhost:11434".into());
        assert!(matches!(c.validate(), Err(AiError::Transport(_))));
        c.base_url = Some("http://localhost:11434/v1".into());
        assert_eq!(c.validate(), Ok(()));
        assert!(AiProviderKind::Anthropic.needs_key());
        assert!(!AiProviderKind::OpenaiCompatible.needs_key());
        assert_eq!(
            key_slot(AiProviderKind::OpenaiCompatible).variable,
            "openai-compatible"
        );
    }

    #[test]
    fn runtime_values_that_look_like_credentials_are_secrets() {
        let collection = Collection {
            environments: vec![rt_core::model::Environment {
                name: "Local".into(),
                variables: vec![Variable {
                    name: "pin".into(),
                    value: None,
                    enabled: true,
                    secret: true,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let input = SendInput {
            environment: Some("Local".into()),
            vars: vec![
                ("pin".into(), "1234".into()),
                ("sessionToken".into(), "abc".into()),
                ("id".into(), "7".into()),
                ("base".into(), "https://api.example.com/v1/long".into()),
                ("trace".into(), "0123456789abcdef0123".into()),
            ],
            secrets: vec![("password".into(), "hunter2".into())],
        };
        assert_eq!(
            secrets_in_scope(&collection, &input),
            ["0123456789abcdef0123", "1234", "abc", "hunter2"]
        );
    }

    /// A provider that answers with a fixed text and keeps the calls.
    struct Canned(std::cell::RefCell<Vec<AiCall>>, String);

    impl AiProvider for Canned {
        fn complete<'a>(&'a self, call: AiCall) -> LocalBoxFuture<'a, Result<String, AiError>> {
            self.0.borrow_mut().push(call);
            let answer = self.1.clone();
            Box::pin(async move { Ok(answer) })
        }
    }

    #[test]
    fn provider_errors_are_masked() {
        struct Failing;
        impl AiProvider for Failing {
            fn complete<'a>(&'a self, _: AiCall) -> LocalBoxFuture<'a, Result<String, AiError>> {
                Box::pin(async { Err(AiError::Provider("401 bad key topsecret".into())) })
            }
        }
        let task = AiTask::CreateRequest {
            description: "x".into(),
            folder: String::new(),
        };
        let err = block_on(run(
            &task,
            &AiContext::default(),
            &Failing,
            &["topsecret".into()],
        ))
        .unwrap_err();
        assert_eq!(err, AiError::Provider("401 bad key ***".into()));
    }

    #[test]
    fn the_call_is_masked_and_the_answer_parsed() {
        let ctx = AiContext {
            collection: "shop s3cr3t-value".into(),
            ..Default::default()
        };
        let p = Canned(
            Default::default(),
            "```json\n{\"text\": \"ok\", \"request\": {\"method\": \"get\", \"url\": \"{{base}}/x\"}}\n```"
                .into(),
        );
        let task = AiTask::CreateRequest {
            description: "with \"s3cr3t-value\" in it".into(),
            folder: String::new(),
        };
        let answer = block_on(run(&task, &ctx, &p, &["s3cr3t-value".into()])).unwrap();
        assert_eq!(answer.request.unwrap().method, "GET");
        let calls = p.0.borrow();
        let json = serde_json::to_string(&calls[0]).unwrap();
        assert!(!json.contains("s3cr3t"), "{json}");
        assert!(json.contains("***"));
    }

    pub(super) fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }
}
