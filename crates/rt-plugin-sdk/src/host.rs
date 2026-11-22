// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Host calls (`API.md`, "Host calls and permissions") as plain functions. Each one sends an
//! `api::HostCall` through the `rt.host` import and waits for the reply; calls that take time,
//! like [`http`], look synchronous because the host suspends the plugin meanwhile.
//!
//! In native builds (tests) the calls go to the [`MockHost`](crate::testing::MockHost) of the
//! current thread, or fail with "no host" if there is none.

use rt_core::services::NoteLevel;
use rt_plugin::api::{HostCall, PluginRequest, PluginResponse};
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// Sends one host call and returns the `ok` value of the reply.
pub fn call<T: DeserializeOwned>(call: &HostCall) -> Result<T> {
    let request = serde_json::to_vec(call)?;
    let reply = crate::sys::host(&request)?;
    let reply: serde_json::Value = serde_json::from_slice(&reply)
        .map_err(|e| Error::new(format!("invalid host reply: {e}")))?;
    if let Some(error) = reply.get("error").and_then(|e| e.as_str()) {
        return Err(Error::new(error));
    }
    let ok = reply.get("ok").cloned().unwrap_or(serde_json::Value::Null);
    serde_json::from_value(ok).map_err(|e| Error::new(format!("invalid host reply: {e}")))
}

/// Writes to the plugin log the shell shows (developer console, `rt` with `--verbose`).
/// Never fails; a log line that cannot be delivered is dropped.
pub fn log(level: NoteLevel, message: impl Into<String>) {
    let _ = call::<serde_json::Value>(&HostCall::Log {
        level,
        message: message.into(),
    });
}

/// The host's clock: Unix time in milliseconds.
pub fn now() -> Result<u64> {
    call(&HostCall::Now)
}

/// Sends an HTTP request. Needs `permissions.network` to cover the URL's host; otherwise the
/// host replies `permission denied: network` and nothing goes on the wire.
pub fn http(request: PluginRequest) -> Result<PluginResponse> {
    call(&HostCall::Http {
        request,
        credentials: false,
        session: false,
    })
}

/// Like [`http`], with the auth of the request the current hook runs for (header, token or
/// signature, applied by the host; the plugin never sees the values). Only in the `prepare`
/// and `response` hooks.
pub fn http_with_credentials(request: PluginRequest) -> Result<PluginResponse> {
    call(&HostCall::Http {
        request,
        credentials: true,
        session: false,
    })
}

/// Like [`http_with_credentials`], and in the session of the send: cookies of the
/// environment's jar go along and `Set-Cookie` is stored there (e.g. a CSRF token fetch whose
/// token is bound to the session cookie). Only in the `prepare` and `response` hooks, only to
/// the request's origin. Added in wave 6.
pub fn http_in_session(request: PluginRequest) -> Result<PluginResponse> {
    call(&HostCall::Http {
        request,
        credentials: true,
        session: true,
    })
}

/// Reads from the plugin's private store (needs `permissions.storage`).
pub fn storage_get(key: impl Into<String>) -> Result<Option<serde_json::Value>> {
    let value: serde_json::Value = call(&HostCall::StorageGet { key: key.into() })?;
    Ok((!value.is_null()).then_some(value))
}

/// Writes to the plugin's private store (needs `permissions.storage`).
pub fn storage_set(key: impl Into<String>, value: serde_json::Value) -> Result<()> {
    call::<serde_json::Value>(&HostCall::StorageSet {
        key: key.into(),
        value,
    })
    .map(drop)
}

/// Deletes from the plugin's private store (needs `permissions.storage`).
pub fn storage_delete(key: impl Into<String>) -> Result<()> {
    call::<serde_json::Value>(&HostCall::StorageDelete { key: key.into() }).map(drop)
}
