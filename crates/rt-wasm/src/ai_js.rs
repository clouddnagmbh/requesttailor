// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The AI assistant in the browser (wave 8 strand E, wave 16 strand A).
//!
//! The web shell keeps the settings (`localStorage`) and the keys (encrypted browser store);
//! this module is the core around them, the same code as the desktop app:
//!
//! ```ts
//! // free functions
//! aiSettingsNormalize(next: AiSettings, old: AiSettings | null): AiSettings  // validates
//! aiSettingsConfirm(settings: AiSettings, profile: string): AiSettings
//! aiSettingsParse(json: string): AiSettings | null    // also a wave-8 `AiConfig`
//! aiCancel(id: string): void
//! // Workspace methods; `settings` is what the shell stored, `extra` the stored OAuth tokens
//! // (for masking only), `deviceOff` whether this device turned the assistant off for the
//! // collection, `companion` { url, token } to go through `rt agent`.
//! aiPolicy(deviceOff): AiCollectionPolicy
//! aiPreview(what: AiWhat, input, settings, extra, deviceOff): AiPreview
//! aiAsk(task, input, settings, key, extra, deviceOff, companion): Promise<AiAnswer>
//! aiComplete(request, input, settings, key, extra, deviceOff, companion,
//!            onEvent: (e: AiEvent) => void): Promise<AiReply>
//! ```
//!
//! Calls go out with `fetch`, or with `companion` through `rt agent`. Rejects with an `Error`
//! named `AppError` whose `kind` is an `AiError` kind (`not-configured`, `no-key`,
//! `transport`, `provider`, `answer`, `unsupported`, `cancelled`, `opted-out`) or an
//! `AppError` kind.

use js_sys::{Function, Promise};
use rt_app::SendInput;
use rt_app::ai::{
    self, AiError, AiEvent, AiProfile, AiReply, AiRequest, AiSettings, AiTask, AiWhat, Calls,
    CancelToken,
};
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use std::sync::OnceLock;
use wasm_bindgen::prelude::*;

use crate::Companion;
use crate::workspace_js::{WorkspaceJs, from_js, named_error, to_js};

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

fn calls() -> &'static Calls {
    static CALLS: OnceLock<Calls> = OnceLock::new();
    CALLS.get_or_init(Calls::default)
}

/// Checks `next` and carries over the confirmed first-use previews of `old` for the profiles
/// that did not move.
#[wasm_bindgen(js_name = aiSettingsNormalize)]
pub fn ai_settings_normalize(next: JsValue, old: JsValue) -> Result<JsValue, JsValue> {
    let mut next: AiSettings = from_js(&next, "settings")?;
    let old: Option<AiSettings> = optional(&old, "old settings")?;
    next.validate().map_err(ai_error)?;
    next.carry_previewed(old.as_ref());
    to_js(&next)
}

/// `settings` with the first-use preview of `profile` confirmed.
#[wasm_bindgen(js_name = aiSettingsConfirm)]
pub fn ai_settings_confirm(settings: JsValue, profile: String) -> Result<JsValue, JsValue> {
    let mut s: AiSettings = from_js(&settings, "settings")?;
    s.confirm(&profile);
    to_js(&s)
}

/// Reads stored settings: wave-16 settings or a wave-8 config; `null` for anything else.
#[wasm_bindgen(js_name = aiSettingsParse)]
pub fn ai_settings_parse(json: &str) -> Result<JsValue, JsValue> {
    match AiSettings::parse(json.as_bytes()) {
        Some(s) => to_js(&s),
        None => Ok(JsValue::NULL),
    }
}

/// Stops the call `id` if it runs.
#[wasm_bindgen(js_name = aiCancel)]
pub fn ai_cancel(id: &str) {
    calls().cancel(id);
}

struct Args {
    settings: Option<AiSettings>,
    input: SendInput,
    extra: Vec<String>,
    device_off: bool,
}

fn args(
    settings: &JsValue,
    input: &JsValue,
    extra: &JsValue,
    device_off: bool,
) -> Result<Args, JsValue> {
    Ok(Args {
        settings: optional(settings, "settings")?,
        input: from_js(input, "input")?,
        extra: optional(extra, "extra secrets")?.unwrap_or_default(),
        device_off,
    })
}

fn key_check(profile: &AiProfile, key: &Option<String>) -> Result<(), JsValue> {
    if profile.config.provider.needs_key() && key.as_deref().is_none_or(|k| k.trim().is_empty()) {
        return Err(ai_error(AiError::NoKey));
    }
    Ok(())
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// Whether the assistant may be used for this collection.
    #[wasm_bindgen(js_name = aiPolicy)]
    pub fn ai_policy(&self, device_off: bool) -> Result<JsValue, JsValue> {
        to_js(&ai::policy(self.workspace().collection(), device_off))
    }

    /// Exactly what `what` would send, masked; nothing leaves.
    #[wasm_bindgen(js_name = aiPreview)]
    pub fn ai_preview(
        &self,
        what: JsValue,
        input: JsValue,
        settings: JsValue,
        extra: JsValue,
        device_off: bool,
    ) -> Result<JsValue, JsValue> {
        let what: AiWhat = from_js(&what, "what")?;
        let a = args(&settings, &input, &extra, device_off)?;
        let prepared = self.prepare(&what, &a)?;
        let settings = a.settings.ok_or_else(|| ai_error(AiError::NotConfigured))?;
        let shown = ai::preview(
            &settings,
            &prepared.profile,
            prepared.call,
            &prepared.secrets,
        )
        .map_err(ai_error)?;
        to_js(&shown)
    }

    /// One task of the app (see the module documentation).
    #[wasm_bindgen(js_name = aiAsk)]
    pub fn ai_ask(
        &self,
        task: JsValue,
        input: JsValue,
        settings: JsValue,
        key: Option<String>,
        extra: JsValue,
        device_off: bool,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let task: AiTask = from_js(&task, "task")?;
        let a = args(&settings, &input, &extra, device_off)?;
        let prepared = self.prepare(&AiWhat::Task { task: task.clone() }, &a)?;
        key_check(&prepared.profile, &key)?;
        let settings = a.settings.ok_or_else(|| ai_error(AiError::NotConfigured))?;
        let companion: Option<Companion> = optional(&companion, "companion")?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let cancel = CancelToken::default();
            let answer = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    ai::ask_with(&t, &settings, prepared, &task, key, false, &cancel).await
                }
                None => {
                    let t = FetchTransport::new();
                    ai::ask_with(&t, &settings, prepared, &task, key, true, &cancel).await
                }
            };
            to_js(&answer.map_err(ai_error)?)
        }))
    }

    /// A call of an extension, streamed to `on_event`; see the module documentation.
    #[wasm_bindgen(js_name = aiComplete)]
    pub fn ai_complete(
        &self,
        request: JsValue,
        input: JsValue,
        settings: JsValue,
        key: Option<String>,
        extra: JsValue,
        device_off: bool,
        companion: JsValue,
        on_event: Function,
    ) -> Result<Promise, JsValue> {
        let request: AiRequest = from_js(&request, "request")?;
        let id = request.id.clone();
        let a = args(&settings, &input, &extra, device_off)?;
        let prepared = self.prepare(&AiWhat::Request { request }, &a)?;
        key_check(&prepared.profile, &key)?;
        let settings = a.settings.ok_or_else(|| ai_error(AiError::NotConfigured))?;
        let shown = ai::preview(
            &settings,
            &prepared.profile,
            prepared.call.clone(),
            &prepared.secrets,
        )
        .map_err(ai_error)?;
        let companion: Option<Companion> = optional(&companion, "companion")?;
        let (token, guard) = calls().start(&id);
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let _guard = guard;
            let emit = |e: &AiEvent| {
                if let Ok(v) = to_js(e) {
                    let _ = on_event.call1(&JsValue::NULL, &v);
                }
            };
            let delta = |d| emit(&AiEvent::Delta { delta: d });
            let result: Result<AiReply, AiError> = match companion {
                Some(c) => {
                    let t = CompanionTransport::new(c.url, c.token);
                    ai::execute(
                        &t,
                        &prepared.profile,
                        key,
                        false,
                        shown.call,
                        &prepared.secrets,
                        &token,
                        &delta,
                    )
                    .await
                }
                None => {
                    let t = FetchTransport::new();
                    ai::execute(
                        &t,
                        &prepared.profile,
                        key,
                        true,
                        shown.call,
                        &prepared.secrets,
                        &token,
                        &delta,
                    )
                    .await
                }
            };
            match result {
                Ok(reply) => {
                    emit(&AiEvent::Done {
                        reply: reply.clone(),
                    });
                    to_js(&reply)
                }
                Err(e) => {
                    emit(&AiEvent::Error { error: e.clone() });
                    Err(ai_error(e))
                }
            }
        }))
    }
}

impl WorkspaceJs {
    fn prepare(&self, what: &AiWhat, a: &Args) -> Result<ai::Prepared, JsValue> {
        let responses = self.responses();
        let workspace = self.workspace();
        ai::prepare_what(
            &workspace,
            &responses,
            a.settings.as_ref(),
            what,
            &a.input,
            a.extra.clone(),
            a.device_off,
        )
        .map_err(ai_error)
    }
}
