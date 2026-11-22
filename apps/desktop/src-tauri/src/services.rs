// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The services the desktop app's sends use (wave 5): puts together the parts each strand
//! owns in its own module (`plugins` A, `auth` C, `cookies` D). `Core` keeps one
//! [`ServiceParts`] (shared, `Send + Sync`); every send builds a [`SendServices`] on its
//! blocking thread and lends it to `SendSnapshot::send_with`.

use std::path::Path;

use rt_core::services::Services;

use crate::{auth, cookies, plugins, scripts, session_vars};

/// Long-lived state: installed plugins, token storage, cookie jars.
pub struct ServiceParts {
    pub plugins: plugins::PluginParts,
    pub auth: auth::AuthParts,
    pub cookies: cookies::CookieParts,
    /// Wave 7: trust and engine (strand A), session variables (strand C).
    pub scripts: scripts::ScriptParts,
    pub session: session_vars::SessionParts,
}

impl ServiceParts {
    /// `data_dir`: the app's data folder; `None` keeps everything in memory (tests).
    pub fn new(data_dir: Option<&Path>) -> Self {
        Self {
            plugins: plugins::PluginParts::new(data_dir),
            auth: auth::AuthParts::new(data_dir),
            cookies: cookies::CookieParts::new(data_dir),
            scripts: scripts::ScriptParts::new(data_dir),
            session: session_vars::SessionParts::default(),
        }
    }

    /// The services for one send from collection `collection` (the environment is in the
    /// hook context and the cookie scope of `send_with`).
    pub fn for_send(&self, collection: &str) -> SendServices<'_> {
        SendServices {
            plugins: self
                .plugins
                .for_send(collection, self.cookies.for_send(collection)),
            auth: self.auth.for_send(collection),
            cookies: self.cookies.for_send(collection),
            scripts: self.scripts.for_send(collection),
        }
    }
}

pub struct SendServices<'a> {
    plugins: plugins::PluginSend<'a>,
    auth: auth::AuthSend<'a>,
    cookies: cookies::CookieSend<'a>,
    scripts: scripts::ScriptSend<'a>,
}

impl SendServices<'_> {
    pub fn services(&self) -> Services<'_> {
        Services {
            tokens: self.auth.tokens(),
            prompt: self.auth.prompt(),
            cookies: self.cookies.jar(),
            hooks: self.plugins.hooks(),
            scripts: self.scripts.runner(),
        }
    }
}
