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
//! # Wave 16 (extended, not replaced)
//!
//! - More providers ([`AiProviderKind`]: OpenAI, Azure OpenAI, Gemini, Mistral, Bedrock) and
//!   several profiles ([`AiSettings`], e.g. "fast" and "smart"); the wave-8 [`AiConfig`] is
//!   one profile. The provider matrix is data: [`AiProviderKind::capabilities`].
//! - Tool use and streaming, provider-neutral ([`AiTool`], [`AiReply`], [`AiDelta`]);
//!   [`AiProvider::reply`] and [`AiProvider::stream`] fall back to [`AiProvider::complete`].
//! - The generic call of extensions ([`AiRequest`], masked with [`mask_call`]) and the "show
//!   what will be sent" preview ([`AiPreview`]).
//! - New tasks of the app itself: a request from a snippet, a response explained,
//!   assertions, names, a description draft.
//! - Per collection off ([`AiCollectionPolicy`]): in `opencollection.yml` or on this device.
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
use serde::{Deserialize, Serialize};

use crate::{AppError, SecretKey, SendInput};

mod answer;
mod context;
mod prompt;
pub mod prompts;
mod runtime;
pub mod snippet;

// Seam N5: providers, profiles, the call, masking and the runtime live in `tk-ai`; the old
// paths stay.
pub use context::AiContext;
pub use runtime::{
    AiWhat, CallGuard, Calls, CancelToken, DEFAULT_MAX_TOKENS, Prepared, ask_with, ensure_allowed,
    execute, policy, prepare_what, preview, request_call, resolve, scope_tasks, task_call,
};
pub use snippet::local_answer;
use tk_ai::mask;
pub use tk_ai::{
    ANTHROPIC_BASE_URL, ANTHROPIC_VERSION, AiCall, AiCapabilities, AiConfig, AiDelta, AiError,
    AiEvent, AiJsonMode, AiMessage, AiPreview, AiPreviewMode, AiProfile, AiProvider,
    AiProviderKind, AiReply, AiRole, AiRoles, AiSettings, AiStop, AiTool, AiToolCall, AiToolResult,
    AiUsage, AnthropicProvider, ChatProvider, DEFAULT_ANTHROPIC_MODEL, GEMINI_BASE_URL,
    GeminiProvider, LEGACY_PROFILE, OpenAiCompatibleProvider, ROLE_FAST, ROLE_SMART, endpoint_of,
    mask_call, mask_error, provider, redact, request_url,
};

/// A call an extension asks for, with RequestTailor's [`AiScope`].
pub type AiRequest = tk_ai::AiRequest<AiScope>;

/// [`SecretKey::collection`] of the provider keys (not a collection: app-wide); the kit's
/// [`tk_ai::key_slot`] space. Stored keys depend on it.
pub const KEY_COLLECTION: &str = "requesttailor:ai";
/// [`SecretKey::environment`] of the provider keys.
pub const KEY_ENVIRONMENT: &str = tk_ai::KEY_GROUP;

/// What of the active collection the core puts into the call, masked (an [`AiContext`]).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiScope {
    /// A request path: its spec as written, variables by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    /// A response of the response store: its masked report, body cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<u64>,
    /// A folder path (`""`: the root): its requests in short form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// Whether the assistant may be used for a collection. Off if either says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiCollectionPolicy {
    /// `extensions.requesttailor.ai.enabled: false` in `opencollection.yml`: the team decided
    /// that this repository never goes to an AI provider.
    pub file_off: bool,
    /// The user turned it off for this collection on this device.
    pub device_off: bool,
}

impl AiCollectionPolicy {
    pub fn allowed(self) -> bool {
        !self.file_off && !self.device_off
    }
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
    /// Wave 16: one request from a pasted snippet (code, API documentation text). cURL goes
    /// through the importer first, without a model.
    RequestFromSnippet { snippet: String, folder: String },
    /// Wave 16: a short summary of a response body.
    ExplainResponse { path: String, response: u64 },
    /// Wave 16: assertions only (no script) from the current response.
    SuggestAssertions { path: String, response: u64 },
    /// Wave 16: names for a request, folder or variable.
    SuggestNames { target: AiNameTarget, path: String },
    /// Wave 16: a description draft (Markdown) for one request.
    DraftDocs { path: String },
}

/// What [`AiTask::SuggestNames`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiNameTarget {
    Request,
    Folder,
    /// The variables a request uses (`path` is the request).
    Variable,
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
    /// Wave 16, `SuggestNames`: proposals, best first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// Wave 16, `DraftDocs`: the description draft (Markdown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

/// Where the key of `provider` is kept in the shell's secret store.
pub fn key_slot(provider: AiProviderKind) -> SecretKey {
    secret_key(tk_ai::key_slot(KEY_COLLECTION, provider))
}

/// Where the key of `profile` is kept (before the provider kind's slot, [`key_slot`]).
pub fn profile_key_slot(profile: &str) -> SecretKey {
    secret_key(tk_ai::profile_key_slot(KEY_COLLECTION, profile))
}

fn secret_key(slot: tk_secrets::SecretSlot) -> SecretKey {
    SecretKey {
        collection: slot.space,
        environment: slot.group,
        variable: slot.name,
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
/// The unmasked call of `task` (the prompt of wave 8), for [`runtime::preview`] to mask.
pub(crate) fn prompt_call(task: &AiTask, context: &AiContext) -> AiCall {
    prompt::build(task, context)
}

/// The message a shell shows for `e`. The kit's opt-out says "project"; RequestTailor's unit
/// is the collection, so its text stays as before the extraction (desktop and web alike).
pub fn error_message(e: &AiError) -> String {
    match e {
        AiError::OptedOut => "the AI assistant is off for this collection".to_owned(),
        e => e.to_string(),
    }
}

pub fn prepare(task: &AiTask, context: &AiContext, secrets: &[String]) -> Result<AiCall, AiError> {
    mask_call(prompt::build(task, context), secrets)
}

/// Reads the model's `text` as the answer to `task` (checked, never repaired).
pub fn parse_answer(task: &AiTask, text: &str) -> Result<AiAnswer, AiError> {
    answer::parse(task, text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::services::LocalBoxFuture;

    #[test]
    fn the_opt_out_keeps_requesttailors_wording() {
        assert_eq!(
            error_message(&AiError::OptedOut),
            "the AI assistant is off for this collection"
        );
        assert_eq!(error_message(&AiError::NoKey), AiError::NoKey.to_string());
    }

    #[test]
    fn key_slots_keep_the_stored_strings() {
        let k = key_slot(AiProviderKind::OpenaiCompatible);
        assert_eq!(
            (&*k.collection, &*k.environment, &*k.variable),
            ("requesttailor:ai", "keys", "openai-compatible")
        );
        let p = profile_key_slot("fast");
        assert_eq!(
            (&*p.collection, &*p.environment, &*p.variable),
            ("requesttailor:ai", "keys", "profile:fast")
        );
        assert!(AiCollectionPolicy::default().allowed());
        assert!(
            !AiCollectionPolicy {
                file_off: true,
                device_off: false
            }
            .allowed()
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
