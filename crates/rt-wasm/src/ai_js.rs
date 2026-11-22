// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The AI assistant in the browser (wave 8, strand E): `Workspace.aiAsk`.
//!
//! ```ts
//! aiAsk(task: AiTask, input: SendInput, config: AiConfig, key: string | null,
//!       extraSecrets: string[], companion: { url, token } | null): Promise<AiAnswer>
//! ```
//!
//! The context is built from this workspace (unsaved edits) and its kept responses, masked
//! with every secret in scope: `input.secrets` (the web shell's encrypted store, as for a
//! send), secret-like runtime values, and `extraSecrets` (the stored OAuth tokens). The call
//! goes out with `fetch` (Anthropic: with the direct browser access header) or, with
//! `companion`, through `rt agent`. Rejects with an `Error` named `AppError` whose `kind` is
//! an `AiError` kind (`not-configured`, `no-key`, `transport`, `provider`, `answer`) or an
//! `AppError` kind.

use js_sys::Promise;
use rt_app::SendInput;
use rt_app::ai::{self, AiConfig, AiContext, AiError, AiTask};
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use wasm_bindgen::prelude::*;

use crate::Companion;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, named_error, to_js};

fn ai_error(e: AiError) -> JsValue {
    let kind = serde_json::to_value(&e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "shell".to_owned());
    named_error(&kind, &e.to_string())
}

fn optional<T: serde::de::DeserializeOwned>(
    value: &JsValue,
    what: &str,
) -> Result<Option<T>, JsValue> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        from_js(value, what).map(Some)
    }
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// See the module documentation.
    #[wasm_bindgen(js_name = aiAsk)]
    pub fn ai_ask(
        &self,
        task: JsValue,
        input: JsValue,
        config: JsValue,
        key: Option<String>,
        extra_secrets: JsValue,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let task: AiTask = from_js(&task, "task")?;
        let input: SendInput = from_js(&input, "input")?;
        let config: AiConfig =
            optional(&config, "config")?.ok_or_else(|| ai_error(AiError::NotConfigured))?;
        let extra: Vec<String> = optional(&extra_secrets, "extraSecrets")?.unwrap_or_default();
        let companion: Option<Companion> = optional(&companion, "companion")?;
        config.validate().map_err(ai_error)?;
        if config.provider.needs_key() && key.as_deref().is_none_or(|k| k.trim().is_empty()) {
            return Err(ai_error(AiError::NoKey));
        }
        let (context, secrets) = {
            let responses = self.responses();
            let workspace = self.workspace();
            let context =
                AiContext::new(&workspace, &responses, &task, &input).map_err(app_error)?;
            let mut secrets = ai::secrets_in_scope(workspace.collection(), &input);
            secrets.extend(extra.into_iter().filter(|s| !s.is_empty()));
            (context, secrets)
        };
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let answer = match companion {
                Some(c) => {
                    let transport = CompanionTransport::new(c.url, c.token);
                    let provider =
                        ai::provider(&transport, &config, key, false).map_err(ai_error)?;
                    ai::run(&task, &context, &*provider, &secrets).await
                }
                None => {
                    let transport = FetchTransport::new();
                    let provider =
                        ai::provider(&transport, &config, key, true).map_err(ai_error)?;
                    ai::run(&task, &context, &*provider, &secrets).await
                }
            };
            to_js(&answer.map_err(ai_error)?)
        }))
    }
}
