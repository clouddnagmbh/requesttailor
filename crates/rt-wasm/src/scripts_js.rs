// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts of one browser send (wave 7, strand A): `services.scripts` of `Workspace.send`.
//!
//! - `true`: the collection is trusted, scripts run in `rt_script::Sandbox` (the engine runs
//!   in wasmi and yields to the page every few milliseconds, so a script never freezes it).
//! - `false`: the collection is not trusted; scripts do not run and the report carries the
//!   note `rt_app::scripts::UNTRUSTED_NOTE` if the request has scripts.
//! - absent (`undefined`/`null`): no scripts, no note.
//!
//! The trust itself lives in the web shell (`apps/web/src/scripts.ts`).

use rt_core::model::Collection;
use rt_core::send::SendReport;
use rt_core::services::ScriptRunner;
use rt_script::Sandbox;
use wasm_bindgen::JsValue;

pub struct ScriptSend {
    sandbox: Option<Sandbox>,
    untrusted: bool,
}

impl ScriptSend {
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let value = if services.is_null() || services.is_undefined() {
            JsValue::UNDEFINED
        } else {
            js_sys::Reflect::get(services, &JsValue::from_str("scripts"))?
        };
        let trusted = match value.as_bool() {
            Some(b) => Some(b),
            None if value.is_null() || value.is_undefined() => None,
            None => {
                return Err(js_sys::TypeError::new("services.scripts must be a boolean").into());
            }
        };
        Ok(Self {
            sandbox: (trusted == Some(true)).then(Sandbox::default),
            untrusted: trusted == Some(false),
        })
    }

    pub fn runner(&self) -> Option<&dyn ScriptRunner> {
        self.sandbox.as_ref().map(|s| s as &dyn ScriptRunner)
    }

    /// After the send: the untrusted note (see the module documentation).
    pub fn finish(&self, collection: &Collection, report: &mut SendReport) {
        if self.untrusted {
            rt_app::scripts::note_untrusted(report, collection);
        }
    }
}
