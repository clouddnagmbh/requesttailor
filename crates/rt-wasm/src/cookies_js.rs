// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar in the browser (wave 5, strand D). `fetch` keeps its own cookies, forbids
//! the `Cookie` header and hides `Set-Cookie`, so the jar applies to sends through the
//! companion only; the web shell (`apps/web/src/cookies.ts`) hands it over only then.
//!
//! The `cookies` field of the services object:
//!
//! ```ts
//! { jars: string;               // CookieJars JSON of the collection
//!   save(jars: string): void }  // called after a response changed the jars
//! ```
//!
//! `undefined` or `null`: no jar for this send.

use std::rc::Rc;

use rt_core::cookies::{CookieJars, SharedJars, now_ms};
use rt_core::services::{CookieJar, LocalBoxFuture};
use wasm_bindgen::{JsCast, JsValue};

/// The jar of one send, from the `cookies` field of the services object.
pub struct CookieSend {
    /// Shared with the plugins of the send (`http` with `session: true`, wave 6).
    jar: Option<Rc<JsJar>>,
}

struct JsJar {
    jars: SharedJars,
    save: js_sys::Function,
}

fn field(object: &JsValue, name: &str) -> Result<JsValue, JsValue> {
    if object.is_null() || object.is_undefined() {
        return Ok(JsValue::UNDEFINED);
    }
    js_sys::Reflect::get(object, &JsValue::from_str(name))
}

fn type_error(message: &str) -> JsValue {
    js_sys::TypeError::new(message).into()
}

impl CookieSend {
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let cookies = field(services, "cookies")?;
        if cookies.is_null() || cookies.is_undefined() {
            return Ok(Self { jar: None });
        }
        let jars = field(&cookies, "jars")?
            .as_string()
            .ok_or_else(|| type_error("services.cookies.jars must be a JSON string"))?;
        let save = field(&cookies, "save")?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| type_error("services.cookies.save must be a function"))?;
        Ok(Self {
            jar: Some(Rc::new(JsJar {
                jars: SharedJars::new(CookieJars::from_json(jars.as_bytes())),
                save,
            })),
        })
    }

    pub fn jar(&self) -> Option<&dyn rt_core::services::CookieJar> {
        self.jar.as_deref().map(|j| j as &dyn CookieJar)
    }

    /// The same jar, for the plugins of the send.
    pub fn shared(&self) -> Option<Rc<dyn CookieJar>> {
        self.jar.clone().map(|j| j as Rc<dyn CookieJar>)
    }
}

impl CookieJar for JsJar {
    fn cookie_header<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
    ) -> LocalBoxFuture<'a, Option<String>> {
        self.jars.cookie_header(scope, url)
    }

    fn store<'a>(
        &'a self,
        scope: &'a str,
        url: &'a str,
        set_cookie: &'a [String],
    ) -> LocalBoxFuture<'a, ()> {
        let now = now_ms();
        let (changed, json) = self.jars.with(|jars| {
            let before = jars.clone();
            jars.store(scope, url, set_cookie, now);
            (*jars != before, jars.to_json())
        });
        if changed {
            let json = String::from_utf8(json).unwrap_or_default();
            // A failing callback must not fail the send; the shell logs its own errors.
            let _ = self.save.call1(&JsValue::NULL, &JsValue::from_str(&json));
        }
        Box::pin(std::future::ready(()))
    }
}
