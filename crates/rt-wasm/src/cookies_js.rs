// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar in the browser (wave 5, strand D). `fetch` keeps its own cookies, so the
//! jar applies to sends through the companion only. Interface commit: no jar. Strand D owns
//! this file.

use wasm_bindgen::JsValue;

/// The jar of one send, from the `cookies` field of the services object.
pub struct CookieSend {
    _private: (),
}

impl CookieSend {
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let _ = services;
        Ok(Self { _private: () })
    }

    pub fn jar(&self) -> Option<&dyn rt_core::services::CookieJar> {
        None
    }
}
