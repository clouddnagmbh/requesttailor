// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The send guard of one browser send (wave 10): `services.guard` of `Workspace.send`, a
//! function `(check) => verdict` the web shell takes from the distribution
//! (`createWebBackend({ sendGuard })`). `check` is the JSON of `rt_core::guard::SendCheck`,
//! the result that of `GuardVerdict` (`{ decision: "allow" }` or
//! `{ decision: "refuse", rule, reason }`). Absent: no guard (the builds of this repository).
//!
//! Fails closed: a guard that throws or answers something else refuses the request.

use rt_core::guard::{GuardVerdict, SendCheck, SendGuard};
use wasm_bindgen::{JsCast as _, JsValue};

pub struct GuardSend {
    function: Option<js_sys::Function>,
}

impl GuardSend {
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        if services.is_null() || services.is_undefined() {
            return Ok(Self { function: None });
        }
        let value = js_sys::Reflect::get(services, &JsValue::from_str("guard"))?;
        if value.is_null() || value.is_undefined() {
            return Ok(Self { function: None });
        }
        match value.dyn_into::<js_sys::Function>() {
            Ok(function) => Ok(Self {
                function: Some(function),
            }),
            Err(_) => Err(js_sys::TypeError::new("services.guard must be a function").into()),
        }
    }

    pub fn guard(&self) -> Option<&dyn SendGuard> {
        self.function.as_ref().map(|_| self as &dyn SendGuard)
    }
}

impl SendGuard for GuardSend {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        let refuse = |reason: &str| GuardVerdict::Refuse {
            rule: "guard".into(),
            reason: reason.into(),
        };
        let Some(f) = &self.function else {
            return GuardVerdict::Allow;
        };
        let Ok(json) = serde_json::to_string(check) else {
            return refuse("the send guard could not be asked");
        };
        let Ok(arg) = js_sys::JSON::parse(&json) else {
            return refuse("the send guard could not be asked");
        };
        let answer = match f.call1(&JsValue::NULL, &arg) {
            Ok(a) => a,
            Err(_) => return refuse("the send guard failed"),
        };
        js_sys::JSON::stringify(&answer)
            .ok()
            .and_then(|s| s.as_string())
            .and_then(|s| serde_json::from_str::<GuardVerdict>(&s).ok())
            .unwrap_or_else(|| refuse("the send guard gave no valid answer"))
    }
}
