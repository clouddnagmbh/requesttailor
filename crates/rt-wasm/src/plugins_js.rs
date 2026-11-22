// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin host in the browser (wave 5, strand A): `rt_plugin::host::PluginHost` behind a
//! JS class, with host services over `fetch` (or the companion) and browser storage.
//! Interface commit: empty. Strand A owns this file.

use wasm_bindgen::JsValue;

/// The plugin part of one send, from the `plugins` field of the services object the web
/// shell passes to `Workspace.send`.
pub struct PluginSend {
    _private: (),
}

impl PluginSend {
    pub fn from_js(services: &JsValue) -> Result<Self, JsValue> {
        let _ = services;
        Ok(Self { _private: () })
    }

    pub fn hooks(&self) -> Option<&dyn rt_core::services::RequestHooks> {
        None
    }
}
