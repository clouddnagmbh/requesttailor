// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One `rt_call`: `api::Call` JSON in, `api::Reply` JSON out. Pure (no pointers), so native
//! tests run exactly what the WebAssembly export runs.

use rt_plugin::api::{Call, DetectReply};
use serde::Serialize;

use crate::plugin::{Action, Plugin, Tab};
use crate::{Error, Result};

/// Handles one call (`api::Call` as JSON) and returns the `api::Reply` as JSON: `{"ok": …}`
/// or `{"error": "…"}`. Never panics on bad input; a hook that panics traps the module (the
/// host reports it as a failed plugin).
pub fn handle<P: Plugin + ?Sized>(plugin: &P, call: &[u8]) -> Vec<u8> {
    let reply = match serde_json::from_slice::<Call>(call) {
        Ok(call) => run(plugin, call),
        Err(e) => Err(invalid_call(call, e)),
    };
    let value = match reply {
        Ok(ok) => serde_json::json!({ "ok": ok }),
        Err(e) => serde_json::json!({ "error": e.into_message() }),
    };
    serde_json::to_vec(&value).expect("JSON values always serialize")
}

fn run<P: Plugin + ?Sized>(plugin: &P, call: Call) -> Result<serde_json::Value> {
    match call {
        Call::Prepare { request, context } => json(plugin.prepare(request, context)?),
        Call::Response {
            request,
            response,
            context,
        } => json(plugin.response(request, response, context)?),
        Call::RenderTab {
            tab,
            target,
            spec,
            response,
            state,
        } => json(plugin.render_tab(Tab {
            tab,
            target,
            spec,
            response,
            state,
        })?),
        Call::Action {
            tab,
            action,
            target,
            spec,
            response,
            state,
            input,
        } => json(plugin.action(Action {
            tab,
            action,
            target,
            spec,
            response,
            state,
            input,
        })?),
        Call::Detect { importer, files } => json(DetectReply {
            matches: plugin.detect(&importer, files)?,
        }),
        Call::Import { importer, files } => json(plugin.import(&importer, files)?),
    }
}

fn json(value: impl Serialize) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(value)?)
}

/// A hook this SDK version does not know (from a newer host) gets a clear message instead of
/// a serde error.
fn invalid_call(call: &[u8], e: serde_json::Error) -> Error {
    let hook = serde_json::from_slice::<serde_json::Value>(call)
        .ok()
        .and_then(|v| v.get("hook")?.as_str().map(str::to_owned));
    match hook {
        Some(hook) if !KNOWN_HOOKS.contains(&hook.as_str()) => {
            Error::new(format!("unsupported hook: {hook}"))
        }
        _ => Error::new(format!("invalid call: {e}")),
    }
}

const KNOWN_HOOKS: [&str; 6] = [
    "prepare",
    "response",
    "render-tab",
    "action",
    "detect",
    "import",
];
