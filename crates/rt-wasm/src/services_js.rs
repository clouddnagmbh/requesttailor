// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The services of one browser send (wave 5): the web shell passes one object to
//! `Workspace.send` (`{ plugins?, auth?, cookies?, scripts?, guard? }`, see
//! `apps/web/src/send-services.ts`);
//! each strand reads its own field in its own module.

use rt_core::services::Services;
use wasm_bindgen::JsValue;

use crate::auth_js::AuthSend;
use crate::cookies_js::CookieSend;
use crate::guard_js::GuardSend;
use crate::plugins_js::PluginSend;
use crate::scripts_js::ScriptSend;

pub struct SendServices {
    plugins: PluginSend,
    auth: AuthSend,
    cookies: CookieSend,
    /// Wave 7.
    pub scripts: ScriptSend,
    /// Wave 10: the distribution's send guard (`services.guard`).
    guard: GuardSend,
}

impl SendServices {
    /// `null` or `undefined`: no services.
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let cookies = CookieSend::from_js(services)?;
        Ok(Self {
            plugins: PluginSend::from_js(services, cookies.shared())?,
            auth: AuthSend::from_js(services)?,
            cookies,
            scripts: ScriptSend::from_js(services)?,
            guard: GuardSend::from_js(services)?,
        })
    }

    pub fn services(&self) -> Services<'_> {
        Services {
            tokens: self.auth.tokens(),
            prompt: self.auth.prompt(),
            cookies: self.cookies.jar(),
            hooks: self.plugins.hooks(),
            scripts: self.scripts.runner(),
            guard: self.guard.guard(),
            caller: rt_core::guard::Caller::App,
            // The web app has no distribution observers: its extensions hear sends through the
            // UI's `AppExtension.onExchange`, and a companion (`rt agent`) delivers to its own
            // observers. The sequence recorder (wave 19) is the one observer of the page.
            observers: Some(crate::sequence_js::observers()),
            snapshots: None,
        }
    }
}
