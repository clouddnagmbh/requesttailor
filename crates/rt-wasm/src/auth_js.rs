// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 in the browser (wave 5, strand C): `TokenStore` and `AuthPrompt` implemented
//! over JS callbacks of the web shell, so `send_with` runs the same flows as natively.
//! Interface commit: empty. Strand C owns this file.

use wasm_bindgen::JsValue;

/// Token cache and prompts of one send, from the `auth` field of the services object.
pub struct AuthSend {
    _private: (),
}

impl AuthSend {
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let _ = services;
        Ok(Self { _private: () })
    }

    pub fn tokens(&self) -> Option<&dyn rt_core::services::TokenStore> {
        None
    }

    pub fn prompt(&self) -> Option<&dyn rt_core::services::AuthPrompt> {
        None
    }
}
