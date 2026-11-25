// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand B: guided network access for extensions in the browser
//! (`rt_app::egress`). The web shell keeps the list of allowed hosts (browser storage); these
//! free functions are the core around it, the same code as the desktop app:
//!
//! ```text
//! egressHostsParse(text): EgressHost[]                   // rejects a broken file
//! egressHostsAllow(hosts, host, port | null, extension, purpose, nowMs): { hosts, entry }
//! egressHostsRemove(hosts, host, port | null): EgressHost[]
//! egressSecretNames(request): string[]                   // the {{secret:<name>}} of the headers
//! egressRequest(request, hosts, secrets: [name, value][], collection, companion | null,
//!               services | null): Promise<EgressResponse>
//! ```
//!
//! `egressRequest` goes out with `fetch`, or with `companion` `{ url, token }` through `rt agent`
//! (the call is marked for the companion, which then keeps it away from its exchange
//! observers). When the browser refuses (CORS), the promise rejects with kind `transport` and a
//! message that starts with `blocked by the browser`; the shell then repeats the call over the
//! companion if one is paired. `services.guard` is the distribution's send guard (as for a
//! send; it answers every call with `Purpose::Egress`). Errors are `Error`s named `AppError`
//! whose `kind` is `not-allowed`, `blocked`, `limit` or `transport`.
#![cfg(target_arch = "wasm32")]

use js_sys::Promise;
use rt_app::egress::{self, Context, EgressError, EgressHost, EgressRequest, HostList};
use rt_core::guard::Caller;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use rt_core::transport::{HttpRequest, Response, Transport, TransportError};
use wasm_bindgen::prelude::*;

use crate::Companion;
use crate::guard_js::GuardSend;
use crate::workspace_js::{from_js, named_error, to_js};

/// The header that tells the companion this is an egress call (it removes the header and does
/// not hand the exchange to its observers). Defined in `rt_core::transport::companion`.
use rt_core::transport::companion::EGRESS_HEADER;

fn egress_error(e: &EgressError) -> JsValue {
    let kind = serde_json::to_value(e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "transport".to_owned());
    named_error(&kind, &e.to_string())
}

fn host_error(e: egress::HostError) -> JsValue {
    named_error("shell", &e.to_string())
}

fn optional_port(port: &JsValue) -> Result<Option<u16>, JsValue> {
    if port.is_null() || port.is_undefined() {
        return Ok(None);
    }
    port.as_f64()
        .filter(|p| p.fract() == 0.0 && (1.0..=65535.0).contains(p))
        .map(|p| Some(p as u16))
        .ok_or_else(|| js_sys::TypeError::new("port must be a number from 1 to 65535").into())
}

fn list_from(hosts: &JsValue) -> Result<HostList, JsValue> {
    let json = js_sys::JSON::stringify(hosts)
        .map_err(|_| js_sys::TypeError::new("hosts must be JSON-serializable"))?;
    HostList::from_json(&String::from(json)).map_err(host_error)
}

/// Reads the stored text of the list; rejects when it is not a list of hosts.
#[wasm_bindgen(js_name = egressHostsParse)]
pub fn egress_hosts_parse(text: &str) -> Result<JsValue, JsValue> {
    let list = HostList::from_json(text).map_err(host_error)?;
    to_js(&list.list())
}

/// `hosts` with `host` allowed (an entry that is there stays); `{ hosts, entry }`.
#[wasm_bindgen(js_name = egressHostsAllow)]
pub fn egress_hosts_allow(
    hosts: JsValue,
    host: &str,
    port: JsValue,
    extension: &str,
    purpose: &str,
    now_ms: f64,
) -> Result<JsValue, JsValue> {
    let mut list = list_from(&hosts)?;
    let entry = list
        .allow(
            host,
            optional_port(&port)?,
            extension,
            purpose,
            now_ms as i64,
        )
        .map_err(host_error)?;
    #[derive(serde::Serialize)]
    struct Allowed<'a> {
        hosts: &'a [EgressHost],
        entry: EgressHost,
    }
    to_js(&Allowed {
        hosts: list.list(),
        entry,
    })
}

/// `hosts` without the entry for `host` and `port`.
#[wasm_bindgen(js_name = egressHostsRemove)]
pub fn egress_hosts_remove(hosts: JsValue, host: &str, port: JsValue) -> Result<JsValue, JsValue> {
    let mut list = list_from(&hosts)?;
    list.remove(host, optional_port(&port)?);
    to_js(&list.list())
}

/// The secret names the headers of `request` refer to.
#[wasm_bindgen(js_name = egressSecretNames)]
pub fn egress_secret_names(request: JsValue) -> Result<JsValue, JsValue> {
    let request: EgressRequest = from_js(&request, "request")?;
    to_js(&egress::secret_names(&request))
}

/// Marks every request for the companion as an egress call.
struct Marked(CompanionTransport);

impl Transport for Marked {
    async fn send(&self, mut request: HttpRequest) -> Result<Response, TransportError> {
        request
            .headers
            .push((EGRESS_HEADER.to_owned(), "1".to_owned()));
        self.0.send(request).await
    }
}

/// One call; see the module documentation.
#[wasm_bindgen(js_name = egressRequest)]
pub fn egress_request(
    request: JsValue,
    hosts: JsValue,
    secrets: JsValue,
    collection: String,
    companion: JsValue,
    services: JsValue,
) -> Result<Promise, JsValue> {
    let request: EgressRequest = from_js(&request, "request")?;
    let hosts = list_from(&hosts)?;
    let secrets: Vec<(String, String)> = from_js(&secrets, "secrets")?;
    let companion: Option<Companion> = if companion.is_null() || companion.is_undefined() {
        None
    } else {
        Some(from_js(&companion, "companion")?)
    };
    let guard = GuardSend::from_js(&services)?;
    Ok(wasm_bindgen_futures::future_to_promise(async move {
        let ctx = Context {
            hosts: &hosts,
            guard: guard.guard(),
            caller: Caller::App,
            collection: &collection,
            environment: request.environment.as_deref(),
            secrets: &secrets,
        };
        let answer = match companion {
            Some(c) => {
                let transport = Marked(CompanionTransport::new(c.url, c.token));
                egress::request(&transport, &ctx, &request).await
            }
            None => egress::request(&FetchTransport::new(), &ctx, &request).await,
        };
        answer.map_err(|e| egress_error(&e)).and_then(|a| to_js(&a))
    }))
}
