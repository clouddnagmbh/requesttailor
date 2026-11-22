// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 in the browser (wave 5, strand C): `TokenStore` and `AuthPrompt` implemented
//! over JS callbacks of the web shell, so `send_with` runs the same flows as natively.
//!
//! The `auth` field of the services object (`apps/web/src/auth.ts`, `forSend`):
//!
//! | Callback | |
//! | --- | --- |
//! | `get(key)` | `StoredToken` or `null` (may return a promise) |
//! | `put(key, token)`, `remove(key)` | store in / remove from the encrypted browser store |
//! | `authorize(url, redirectUri)` | the redirect URL the popup returned to; rejects on cancel |
//! | `deviceCode(prompt)`, `deviceCodeDone()` | show / hide the device code |
//! | `cancelled()` | `true` once the user cancelled |
//!
//! A missing `auth` field: no cache, no prompts. Storage failures never stop a send (the
//! token is fetched again next time).

use js_sys::{Function, Promise, Reflect};
use rt_core::services::{
    AuthPrompt, DeviceCodePrompt, LocalBoxFuture, Services, StoredToken, TokenStore,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::workspace_js::{from_js, to_js};

/// Token cache and prompts of one send, from the `auth` field of the services object.
pub struct AuthSend {
    callbacks: Option<Callbacks>,
}

struct Callbacks {
    this: JsValue,
    get: Function,
    put: Function,
    remove: Function,
    authorize: Option<Function>,
    device_code: Option<Function>,
    device_code_done: Option<Function>,
    cancelled: Option<Function>,
}

fn function(object: &JsValue, name: &str) -> Result<Option<Function>, JsValue> {
    let value = Reflect::get(object, &name.into())?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    value.dyn_into::<Function>().map(Some).map_err(|_| {
        js_sys::TypeError::new(&format!("services.auth.{name} must be a function")).into()
    })
}

fn required(object: &JsValue, name: &str) -> Result<Function, JsValue> {
    function(object, name)?
        .ok_or_else(|| js_sys::TypeError::new(&format!("services.auth.{name} is missing")).into())
}

/// Awaits `value` if it is a promise (or any value: `Promise.resolve`).
async fn settle(value: Result<JsValue, JsValue>) -> Result<JsValue, JsValue> {
    JsFuture::from(Promise::resolve(&value?)).await
}

/// A rejection's readable message (`Error.message`, or the value as a string).
fn message(e: &JsValue) -> String {
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        return String::from(err.message());
    }
    e.as_string().unwrap_or_else(|| "failed".to_owned())
}

impl AuthSend {
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        if services.is_null() || services.is_undefined() {
            return Ok(Self { callbacks: None });
        }
        let auth = Reflect::get(services, &"auth".into())?;
        if auth.is_null() || auth.is_undefined() {
            return Ok(Self { callbacks: None });
        }
        Ok(Self {
            callbacks: Some(Callbacks {
                get: required(&auth, "get")?,
                put: required(&auth, "put")?,
                remove: required(&auth, "remove")?,
                authorize: function(&auth, "authorize")?,
                device_code: function(&auth, "deviceCode")?,
                device_code_done: function(&auth, "deviceCodeDone")?,
                cancelled: function(&auth, "cancelled")?,
                this: auth,
            }),
        })
    }

    pub fn tokens(&self) -> Option<&dyn TokenStore> {
        self.callbacks.as_ref().map(|c| c as &dyn TokenStore)
    }

    /// Only with `authorize`: a shell without one has no way to ask the user.
    pub fn prompt(&self) -> Option<&dyn AuthPrompt> {
        self.callbacks
            .as_ref()
            .filter(|c| c.authorize.is_some())
            .map(|c| c as &dyn AuthPrompt)
    }

    pub fn services(&self) -> Services<'_> {
        Services {
            tokens: self.tokens(),
            prompt: self.prompt(),
            ..Services::none()
        }
    }
}

impl TokenStore for Callbacks {
    fn get<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, Option<StoredToken>> {
        Box::pin(async move {
            let value = settle(self.get.call1(&self.this, &key.into())).await.ok()?;
            if value.is_null() || value.is_undefined() {
                return None;
            }
            from_js(&value, "token").ok()
        })
    }

    fn put<'a>(&'a self, key: &'a str, token: &'a StoredToken) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            if let Ok(value) = to_js(token) {
                let _ = settle(self.put.call2(&self.this, &key.into(), &value)).await;
            }
        })
    }

    fn remove<'a>(&'a self, key: &'a str) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            let _ = settle(self.remove.call1(&self.this, &key.into())).await;
        })
    }
}

impl AuthPrompt for Callbacks {
    fn authorize<'a>(
        &'a self,
        url: &'a str,
        redirect_uri: &'a str,
    ) -> LocalBoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let Some(authorize) = &self.authorize else {
                return Err("this shell cannot show the sign-in".to_owned());
            };
            let value = settle(authorize.call2(&self.this, &url.into(), &redirect_uri.into()))
                .await
                .map_err(|e| message(&e))?;
            value
                .as_string()
                .ok_or_else(|| "the sign-in returned no redirect URL".to_owned())
        })
    }

    fn device_code<'a>(&'a self, prompt: &'a DeviceCodePrompt) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            if let (Some(f), Ok(value)) = (&self.device_code, to_js(prompt)) {
                let _ = settle(f.call1(&self.this, &value)).await;
            }
        })
    }

    fn device_code_done<'a>(&'a self) -> LocalBoxFuture<'a, ()> {
        Box::pin(async move {
            if let Some(f) = &self.device_code_done {
                let _ = settle(f.call0(&self.this)).await;
            }
        })
    }

    fn cancelled(&self) -> bool {
        self.cancelled
            .as_ref()
            .and_then(|f| f.call0(&self.this).ok())
            .is_some_and(|v| v.is_truthy())
    }
}
