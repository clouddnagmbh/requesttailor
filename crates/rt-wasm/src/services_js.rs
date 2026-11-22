// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The services of one browser send (wave 5): the web shell passes one object to
//! `Workspace.send` (`{ plugins?, auth?, cookies? }`, see `apps/web/src/send-services.ts`);
//! each strand reads its own field in its own module.

use rt_core::services::Services;
use wasm_bindgen::JsValue;

use crate::auth_js::AuthSend;
use crate::cookies_js::CookieSend;
use crate::plugins_js::PluginSend;

pub struct SendServices {
    plugins: PluginSend,
    auth: AuthSend,
    cookies: CookieSend,
}

impl SendServices {
    /// `null` or `undefined`: no services.
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let cookies = CookieSend::from_js(services)?;
        Ok(Self {
            plugins: PluginSend::from_js(services, cookies.shared())?,
            auth: AuthSend::from_js(services)?,
            cookies,
        })
    }

    pub fn services(&self) -> Services<'_> {
        Services {
            tokens: self.auth.tokens(),
            prompt: self.auth.prompt(),
            cookies: self.cookies.jar(),
            hooks: self.plugins.hooks(),
        }
    }
}
