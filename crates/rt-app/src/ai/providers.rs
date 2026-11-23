// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The two providers, over the app's own `Transport`. Neither is preferred; the user picks
//! one and gives its key (or a local endpoint).
//!
//! - [`AnthropicProvider`]: the Messages API (`POST /v1/messages`, headers `x-api-key` and
//!   `anthropic-version: 2026-10-21`). For current models it asks for adaptive thinking and
//!   medium effort; on Anthropic's own endpoint, for the models that support it, it turns on
//!   the server-side fallback when a model declines (`fallbacks: "default"`, beta
//!   `server-side-fallback-2026-07-01`). Called from the browser with `fetch`, it sends
//!   `anthropic-dangerous-direct-browser-access: true`, the header Anthropic's SDKs send for
//!   direct browser access (the key is the user's own, kept in their browser).
//! - [`OpenAiCompatibleProvider`]: `POST {base}/chat/completions` of any OpenAI-compatible
//!   server, e.g. Ollama at `http://localhost:11434/v1`; the key is optional.
//!
//! Both answer with the text of the model's reply; errors are masked with the key.

use std::time::Duration;

use bytes::Bytes;
use rt_core::services::LocalBoxFuture;
use rt_core::transport::{HttpBody, HttpRequest, SendOptions, Transport, collect};
use serde_json::{Value, json};

use super::{AiCall, AiConfig, AiError, AiProvider, AiProviderKind, AiRole, redact};

/// Anthropic's public endpoint.
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
/// The `anthropic-version` header.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Models with adaptive thinking and `output_config.effort`.
const ADAPTIVE: &[&str] = &[
    "claude-fable-",
    "claude-mythos-",
    "claude-opus-5",
    "claude-sonnet-5",
    "claude-haiku-5",
    "claude-opus-4-6",
    "claude-opus-4-7",
    "claude-opus-4-8",
    "claude-sonnet-4-6",
];
/// Models with the server-side refusal fallback.
const FALLBACKS: &[&str] = &[
    "claude-fable-5-2",
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];
/// A model call may think for a while.
const TIMEOUT: Duration = Duration::from_secs(300);
/// Largest answer read.
const ANSWER_LIMIT: usize = 8 * 1024 * 1024;

fn options() -> SendOptions {
    SendOptions {
        timeout: Some(TIMEOUT),
        // A redirect could carry the key to another host.
        follow_redirects: false,
        max_redirects: 0,
        client_identity: None,
    }
}

fn trim_base(base: &str) -> &str {
    base.trim().trim_end_matches('/')
}

/// The base URL of `config`, or `default`.
fn base_url(config: &AiConfig, default: &str) -> String {
    match config.base_url.as_deref().map(trim_base) {
        Some(b) if !b.is_empty() => b.to_owned(),
        _ => default.to_owned(),
    }
}

/// Sends `request` and returns the status and the JSON body (or the text, for errors).
async fn exchange<T: Transport>(
    transport: &T,
    request: HttpRequest,
    key: &str,
) -> Result<(u16, Result<Value, String>), AiError> {
    let keys = [key.to_owned()];
    let mask = |s: String| redact(&s, &keys);
    let started = web_time::Instant::now();
    let response = transport
        .send(request)
        .await
        .map_err(|e| AiError::Transport(mask(e.to_string())))?;
    let done = collect(response, ANSWER_LIMIT, started)
        .await
        .map_err(|e| AiError::Transport(mask(e.to_string())))?;
    let status = done.head.status;
    let text = String::from_utf8_lossy(&done.body).into_owned();
    let body = serde_json::from_str::<Value>(&text).map_err(|_| mask(text));
    Ok((status, body))
}

/// `status: message` of an error answer: `error.message` (Anthropic, OpenAI), `error` as a
/// string (Ollama), else the start of the body.
fn error_message(status: u16, body: Result<Value, String>, key: &str) -> AiError {
    let message = match &body {
        Ok(v) => v
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| v.get("error").and_then(Value::as_str))
            .or_else(|| v.get("message").and_then(Value::as_str))
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string()),
        Err(text) => text.clone(),
    };
    let message: String = message.trim().chars().take(300).collect();
    AiError::Provider(redact(&format!("{status} {message}"), &[key.to_owned()]))
}

fn json_request(url: String, mut headers: Vec<(String, String)>, body: &Value) -> HttpRequest {
    headers.push(("accept".into(), "application/json".into()));
    HttpRequest {
        method: "POST".into(),
        url,
        headers,
        body: Some(HttpBody {
            content_type: Some("application/json".into()),
            bytes: Bytes::from(serde_json::to_vec(body).unwrap_or_default()),
        }),
        options: options(),
    }
}

fn role(r: AiRole) -> &'static str {
    match r {
        AiRole::User => "user",
        AiRole::Assistant => "assistant",
    }
}

/// The Messages API.
pub struct AnthropicProvider<'t, T: Transport> {
    transport: &'t T,
    key: String,
    model: String,
    base: String,
    browser: bool,
}

impl<'t, T: Transport> AnthropicProvider<'t, T> {
    /// `key` is required ([`AiError::NoKey`]).
    pub fn new(transport: &'t T, config: &AiConfig, key: Option<String>) -> Result<Self, AiError> {
        let key = key.filter(|k| !k.trim().is_empty()).ok_or(AiError::NoKey)?;
        let base = base_url(config, ANTHROPIC_BASE_URL);
        // `/v1/messages` is appended; a base that already ends in `/v1` is fine too.
        let base = base.strip_suffix("/v1").unwrap_or(&base).to_owned();
        Ok(Self {
            transport,
            key: key.trim().to_owned(),
            model: config.model.trim().to_owned(),
            base,
            browser: false,
        })
    }

    /// Calls straight from a web page (`fetch`): adds the direct browser access header.
    pub fn in_browser(mut self) -> Self {
        self.browser = true;
        self
    }

    fn adaptive(&self) -> bool {
        ADAPTIVE.iter().any(|p| self.model.starts_with(p))
    }

    fn fallbacks(&self) -> bool {
        self.base == ANTHROPIC_BASE_URL && FALLBACKS.contains(&self.model.as_str())
    }

    /// The HTTP request for `call`.
    pub fn request(&self, call: &AiCall) -> HttpRequest {
        let mut body = json!({
            "model": self.model,
            "max_tokens": call.max_tokens,
            "system": call.system,
            "messages": call.messages.iter().map(|m| json!({ "role": role(m.role), "content": m.content })).collect::<Vec<_>>(),
        });
        if self.adaptive() {
            body["thinking"] = json!({ "type": "adaptive" });
            body["output_config"] = json!({ "effort": "medium" });
        }
        let mut headers = vec![
            ("x-api-key".to_owned(), self.key.clone()),
            ("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned()),
        ];
        if self.fallbacks() {
            body["fallbacks"] = json!("default");
            headers.push(("anthropic-beta".into(), FALLBACK_BETA.into()));
        }
        if self.browser {
            headers.push((
                "anthropic-dangerous-direct-browser-access".into(),
                "true".into(),
            ));
        }
        json_request(format!("{}/v1/messages", self.base), headers, &body)
    }
}

impl<T: Transport> AiProvider for AnthropicProvider<'_, T> {
    fn complete<'a>(&'a self, call: AiCall) -> LocalBoxFuture<'a, Result<String, AiError>> {
        Box::pin(async move {
            let (status, body) = exchange(self.transport, self.request(&call), &self.key).await?;
            if status >= 400 {
                return Err(error_message(status, body, &self.key));
            }
            let body = body.map_err(|_| AiError::Answer("the answer was not JSON".into()))?;
            match body.get("stop_reason").and_then(Value::as_str) {
                Some("refusal") => {
                    return Err(AiError::Provider("the model declined to answer".into()));
                }
                Some("max_tokens") => {
                    return Err(AiError::Answer(
                        "the answer was cut off at the token limit".into(),
                    ));
                }
                _ => {}
            }
            let text: Vec<&str> = body
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect();
            if text.is_empty() {
                return Err(AiError::Answer("the answer had no text".into()));
            }
            Ok(text.join("\n"))
        })
    }
}

/// Any `/chat/completions` endpoint.
pub struct OpenAiCompatibleProvider<'t, T: Transport> {
    transport: &'t T,
    key: Option<String>,
    model: String,
    base: String,
}

impl<'t, T: Transport> OpenAiCompatibleProvider<'t, T> {
    /// `config.base_url` is required ([`AiError::NotConfigured`]); `key` is optional.
    pub fn new(transport: &'t T, config: &AiConfig, key: Option<String>) -> Result<Self, AiError> {
        config.validate()?;
        let base = base_url(config, "");
        if base.is_empty() {
            return Err(AiError::NotConfigured);
        }
        Ok(Self {
            transport,
            key: key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()),
            model: config.model.trim().to_owned(),
            base,
        })
    }

    /// The HTTP request for `call`.
    pub fn request(&self, call: &AiCall) -> HttpRequest {
        let mut messages = vec![json!({ "role": "system", "content": call.system })];
        messages.extend(
            call.messages
                .iter()
                .map(|m| json!({ "role": role(m.role), "content": m.content })),
        );
        let body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": call.max_tokens,
            "stream": false,
        });
        let mut headers = Vec::new();
        if let Some(key) = &self.key {
            headers.push(("authorization".to_owned(), format!("Bearer {key}")));
        }
        json_request(format!("{}/chat/completions", self.base), headers, &body)
    }
}

impl<T: Transport> AiProvider for OpenAiCompatibleProvider<'_, T> {
    fn complete<'a>(&'a self, call: AiCall) -> LocalBoxFuture<'a, Result<String, AiError>> {
        Box::pin(async move {
            let key = self.key.clone().unwrap_or_default();
            let (status, body) = exchange(self.transport, self.request(&call), &key).await?;
            if status >= 400 {
                return Err(error_message(status, body, &key));
            }
            let body = body.map_err(|_| AiError::Answer("the answer was not JSON".into()))?;
            let choice = body.pointer("/choices/0");
            if choice
                .and_then(|c| c.get("finish_reason"))
                .and_then(Value::as_str)
                == Some("length")
            {
                return Err(AiError::Answer(
                    "the answer was cut off at the token limit".into(),
                ));
            }
            match choice
                .and_then(|c| c.pointer("/message/content"))
                .and_then(Value::as_str)
            {
                Some(text) if !text.trim().is_empty() => Ok(text.to_owned()),
                _ => Err(AiError::Answer("the answer had no text".into())),
            }
        })
    }
}

/// The provider `config` names, over `transport`. `browser`: called from a web page with
/// `fetch` (Anthropic needs a header for that).
pub fn provider<'t, T: Transport>(
    transport: &'t T,
    config: &AiConfig,
    key: Option<String>,
    browser: bool,
) -> Result<Box<dyn AiProvider + 't>, AiError> {
    config.validate()?;
    Ok(match config.provider {
        AiProviderKind::Anthropic => {
            let p = AnthropicProvider::new(transport, config, key)?;
            Box::new(if browser { p.in_browser() } else { p })
        }
        AiProviderKind::OpenaiCompatible => {
            Box::new(OpenAiCompatibleProvider::new(transport, config, key)?)
        }
        // Wave 16, strand A: their own wire formats (OpenAI, Mistral and Azure share the chat
        // completions shape with their own auth headers; Gemini and Bedrock have their own).
        kind @ (AiProviderKind::Openai
        | AiProviderKind::AzureOpenai
        | AiProviderKind::Gemini
        | AiProviderKind::Mistral
        | AiProviderKind::Bedrock) => {
            return Err(AiError::Unsupported(format!(
                "{} arrives with wave 16",
                kind.label()
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use rt_core::transport::{HttpVersion, Response, ResponseHead, TransportError};

    use super::*;
    use crate::ai::AiMessage;

    struct Once(Option<Bytes>);

    impl futures_core::Stream for Once {
        type Item = Result<Bytes, TransportError>;
        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(self.0.take().map(Ok))
        }
    }

    /// Answers every request with `status` and `body`, keeping the requests.
    struct Fake {
        status: u16,
        body: String,
        seen: RefCell<Vec<HttpRequest>>,
    }

    impl Fake {
        fn new(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.into(),
                seen: RefCell::new(Vec::new()),
            }
        }
        fn sent_json(&self) -> Value {
            let r = &self.seen.borrow()[0];
            serde_json::from_slice(&r.body.as_ref().unwrap().bytes).unwrap()
        }
        fn header(&self, name: &str) -> Option<String> {
            self.seen.borrow()[0]
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        }
    }

    impl Transport for Fake {
        async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
            let url = request.url.clone();
            self.seen.borrow_mut().push(request);
            Ok(Response {
                head: ResponseHead {
                    status: self.status,
                    version: HttpVersion::Http11,
                    headers: vec![],
                    url,
                    redirects: vec![],
                    timing: Default::default(),
                },
                body: Box::pin(Once(Some(Bytes::from(self.body.clone())))),
            })
        }
    }

    fn call() -> AiCall {
        AiCall {
            system: "sys".into(),
            messages: vec![AiMessage::text(AiRole::User, "hi")],
            max_tokens: 100,
            ..Default::default()
        }
    }

    fn config(provider: AiProviderKind, model: &str, base: Option<&str>) -> AiConfig {
        AiConfig {
            provider,
            model: model.into(),
            base_url: base.map(str::to_owned),
        }
    }

    use crate::ai::tests::block_on;

    #[test]
    fn anthropic_messages_request_and_answer() {
        let t = Fake::new(
            200,
            r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"answer"}],"stop_reason":"end_turn"}"#,
        );
        let c = config(AiProviderKind::Anthropic, "claude-opus-5-5", None);
        let p = provider(&t, &c, Some("sk-test".into()), false).unwrap();
        assert_eq!(block_on(p.complete(call())).unwrap(), "answer");
        assert_eq!(
            t.seen.borrow()[0].url,
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(t.header("x-api-key").as_deref(), Some("sk-test"));
        assert_eq!(t.header("anthropic-version").as_deref(), Some("2026-10-21"));
        assert_eq!(t.header("anthropic-beta").as_deref(), Some(FALLBACK_BETA));
        assert_eq!(t.header("anthropic-dangerous-direct-browser-access"), None);
        let body = t.sent_json();
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["system"], "sys");
        assert_eq!(
            body["messages"][0],
            json!({ "role": "user", "content": "hi" })
        );
        assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(body["output_config"]["effort"], "medium");
        assert_eq!(body["fallbacks"], "default");
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn anthropic_in_the_browser_with_an_older_model_and_a_proxy() {
        let t = Fake::new(200, r#"{"content":[{"type":"text","text":"a"}]}"#);
        let c = config(
            AiProviderKind::Anthropic,
            "claude-haiku-4-5",
            Some("https://proxy.example/v1/"),
        );
        let p = provider(&t, &c, Some("k".into()), true).unwrap();
        block_on(p.complete(call())).unwrap();
        assert_eq!(t.seen.borrow()[0].url, "https://proxy.example/v1/messages");
        assert_eq!(
            t.header("anthropic-dangerous-direct-browser-access")
                .as_deref(),
            Some("true")
        );
        assert_eq!(t.header("anthropic-beta"), None);
        let body = t.sent_json();
        assert!(body.get("thinking").is_none() && body.get("fallbacks").is_none());
    }

    #[test]
    fn anthropic_errors() {
        let c = config(AiProviderKind::Anthropic, "claude-opus-5-5", None);
        assert!(matches!(
            provider(&Fake::new(200, ""), &c, None, false).err(),
            Some(AiError::NoKey)
        ));
        let t = Fake::new(
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key sk-live"}}"#,
        );
        let p = provider(&t, &c, Some("sk-live".into()), false).unwrap();
        assert_eq!(
            block_on(p.complete(call())).unwrap_err(),
            AiError::Provider("401 invalid x-api-key ***".into())
        );
        let t = Fake::new(200, r#"{"content":[],"stop_reason":"refusal"}"#);
        let p = provider(&t, &c, Some("k".into()), false).unwrap();
        assert!(matches!(
            block_on(p.complete(call())),
            Err(AiError::Provider(_))
        ));
        let t = Fake::new(
            200,
            r#"{"content":[{"type":"text","text":"a"}],"stop_reason":"max_tokens"}"#,
        );
        let p = provider(&t, &c, Some("k".into()), false).unwrap();
        assert!(matches!(
            block_on(p.complete(call())),
            Err(AiError::Answer(_))
        ));
    }

    #[test]
    fn openai_compatible_with_and_without_a_key() {
        let t = Fake::new(
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}]}"#,
        );
        let c = config(
            AiProviderKind::OpenaiCompatible,
            "llama3.1",
            Some("http://localhost:11434/v1/"),
        );
        let p = provider(&t, &c, None, true).unwrap();
        assert_eq!(block_on(p.complete(call())).unwrap(), "hello");
        assert_eq!(
            t.seen.borrow()[0].url,
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(t.header("authorization"), None);
        let body = t.sent_json();
        assert_eq!(
            body["messages"][0],
            json!({ "role": "system", "content": "sys" })
        );
        assert_eq!(
            body["messages"][1],
            json!({ "role": "user", "content": "hi" })
        );
        assert_eq!(body["model"], "llama3.1");

        let t = Fake::new(500, r#"{"error":"model not found tok-1"}"#);
        let p = provider(&t, &c, Some("tok-1".into()), false).unwrap();
        assert_eq!(
            block_on(p.complete(call())).unwrap_err(),
            AiError::Provider("500 model not found ***".into())
        );
        assert_eq!(t.header("authorization").as_deref(), Some("Bearer tok-1"));

        let no_base = config(AiProviderKind::OpenaiCompatible, "m", None);
        assert!(matches!(
            provider(&t, &no_base, None, false).err(),
            Some(AiError::NotConfigured)
        ));
    }
}
