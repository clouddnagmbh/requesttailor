// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! CSRF tokens per service root (plugin storage `service::storage_key("csrf", root)`, value
//! `{"token": "…", "client": "100" | null}`). A token is fetched with `GET <root>` and
//! `X-CSRF-Token: Fetch` in the session of the send, so it is bound to the same session
//! cookie as the request itself. The `sap-client` of the request goes along; a token of
//! another client is not reused.

use rt_plugin_sdk::api::{PluginNote, PluginRequest, PluginResponse};
use rt_plugin_sdk::{Result, header, host, info, warning};
use serde::{Deserialize, Serialize};

use super::util::{self, MASK};
use crate::service;

pub const HEADER: &str = "X-CSRF-Token";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Cached {
    token: String,
    #[serde(default)]
    client: Option<String>,
}

fn key(root: &str) -> String {
    service::storage_key("csrf", root)
}

/// A usable token value from a response header (not `Required`/`Fetch`, not masked).
fn usable(value: Option<&str>) -> Option<String> {
    let v = value?.trim();
    let usable = !v.is_empty()
        && !v.eq_ignore_ascii_case("required")
        && !v.eq_ignore_ascii_case("fetch")
        && !v.contains(MASK);
    usable.then(|| v.to_owned())
}

/// The token for a modifying request to `root`: cached, or fetched now.
pub fn token(
    request: &PluginRequest,
    root: &str,
    notes: &mut Vec<PluginNote>,
) -> Result<Option<String>> {
    let client = util::query_param(&request.url, "sap-client").filter(|c| !c.contains(MASK));
    if let Some(cached) = host::storage_get(key(root))?
        .and_then(|v| serde_json::from_value::<Cached>(v).ok())
        .filter(|c| c.client == client && !c.token.contains(MASK))
    {
        return Ok(Some(cached.token));
    }
    let url = match &client {
        Some(c) => format!("{root}?sap-client={c}"),
        None => root.to_owned(),
    };
    let fetch = PluginRequest {
        method: "GET".into(),
        url,
        headers: vec![
            (HEADER.into(), "Fetch".into()),
            ("Accept".into(), "application/json".into()),
        ],
        body: None,
    };
    match host::http_in_session(fetch) {
        Ok(response) => match usable(header(&response.headers, HEADER)) {
            Some(token) => {
                store(root, &token, client)?;
                notes.push(info("CSRF token fetched"));
                Ok(Some(token))
            }
            None => {
                notes.push(warning(format!(
                    "CSRF token fetch: the service answered {} without a token",
                    response.status
                )));
                Ok(None)
            }
        },
        Err(e) => {
            notes.push(warning(format!("CSRF token fetch failed: {e}")));
            Ok(None)
        }
    }
}

fn store(root: &str, token: &str, client: Option<String>) -> Result<()> {
    let value = serde_json::to_value(Cached {
        token: token.to_owned(),
        client,
    })?;
    host::storage_set(key(root), value)
}

/// After a response: a `403` with `X-CSRF-Token: Required` drops the cached token; a token
/// the response carries is kept.
pub fn on_response(
    request: &PluginRequest,
    response: &PluginResponse,
    root: &str,
    notes: &mut Vec<PluginNote>,
) -> Result<()> {
    let value = header(&response.headers, HEADER);
    if response.status == 403 && value.is_some_and(|v| v.trim().eq_ignore_ascii_case("required")) {
        host::storage_delete(key(root))?;
        notes.push(warning("CSRF token expired, send again"));
        return Ok(());
    }
    if let Some(token) = usable(value) {
        let client = util::query_param(&request.url, "sap-client").filter(|c| !c.contains(MASK));
        store(root, &token, client)?;
    }
    Ok(())
}
