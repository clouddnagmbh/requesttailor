// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Exchange observers (wave 17): a general extension point that hands every finished send to
//! code a distribution (or a test) adds. It is how an extension learns what came back
//! without owning the transport: record traffic, keep a searchable history, watch for
//! differences.
//!
//! An [`Exchange`] is built **after** the send and **after** masking: its report carries no
//! secret value, not in the URL, headers, cookies or the response body (the same value list
//! as the report, plus header values of `Authorization`, `Cookie`, `Set-Cookie`,
//! `Proxy-Authorization` and of headers whose name contains `key`, `token` or `secret`).
//! The response body is cut at [`EXCHANGE_BODY_LIMIT`].
//!
//! Observers are called synchronously and must be quick: hand the exchange to a channel and
//! return. A slow observer must never slow the send down. The builds of this repository have
//! none. A collection switches delivery off with `extensions.requesttailor.observe.enabled:
//! false` ([`crate::model::ObserveCollectionConfig`]).
//!
//! Where observers hang: `send_with` ([`crate::services::Services::observers`]; app,
//! `rt send`, `rt run`, runner), `rt agent` (every `/v1/send`, `collection` is `None`),
//! `rt mcp` (caller [`Caller::Agent`]), and in the UI `AppExtension.onExchange`.
//!
//! Owner: wave 17 strand A. The types here are the interface.

use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::guard::Caller;
use crate::send::{MASK, ReceivedResponse, ReportBody, SendReport, mask, mask_header};

/// Largest response body an [`Exchange`] carries (1 MiB); longer bodies are cut and
/// [`Exchange::truncated`] is set.
pub const EXCHANGE_BODY_LIMIT: usize = 1024 * 1024;

/// One finished send, masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    /// Who sent.
    pub caller: Caller,
    /// The collection (root path or id as the shell knows it); `None` for `rt agent`, which
    /// sends what the web app hands it.
    pub collection: Option<String>,
    /// The request path in the collection; `None` when the send had none (`rt agent`).
    pub request_path: Option<String>,
    pub environment: Option<String>,
    /// Unix milliseconds when the send finished.
    pub at: i64,
    /// The report with every secret value masked, including the response body (cut at
    /// [`EXCHANGE_BODY_LIMIT`]). Duration and [`crate::transport::Timing`] are in
    /// `report.response`.
    pub report: SendReport,
    /// The response body is incomplete: cut at [`EXCHANGE_BODY_LIMIT`], or already cut by the
    /// send's own body limit.
    #[serde(default)]
    pub truncated: bool,
}

/// Receives every [`Exchange`]. Quick and non-blocking; see the module docs.
pub trait ExchangeObserver {
    fn observe(&self, exchange: &Exchange);
}

/// The observers of a shell, shared between threads. Cheap to clone; compares by identity of
/// its members. Empty by default.
#[derive(Clone, Default)]
pub struct ObserverSet(Vec<Arc<dyn ExchangeObserverSync>>);

/// An observer that can be shared between threads (what [`ObserverSet`] holds).
pub trait ExchangeObserverSync: ExchangeObserver + Send + Sync {}

impl<T: ExchangeObserver + Send + Sync> ExchangeObserverSync for T {}

impl ObserverSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// A set of the given observers.
    pub fn of(observers: Vec<Arc<dyn ExchangeObserverSync>>) -> Self {
        Self(observers)
    }

    /// Adds an observer.
    pub fn with(mut self, observer: Arc<dyn ExchangeObserverSync>) -> Self {
        self.0.push(observer);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Hands `exchange` to every observer, in the order they were added.
    pub fn notify(&self, exchange: &Exchange) {
        for observer in &self.0 {
            observer.observe(exchange);
        }
    }
}

/// Whether the value of a header with this name never reaches an observer.
pub(crate) fn secret_header_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "authorization" | "cookie" | "set-cookie" | "proxy-authorization"
    ) || n.contains("key")
        || n.contains("token")
        || n.contains("secret")
}

fn mask_headers(headers: &mut [(String, String)], secrets: &[String]) {
    for (name, value) in headers {
        *value = if secret_header_name(name) {
            MASK.to_owned()
        } else {
            mask_header(value, secrets)
        };
    }
}

/// Replaces every non-overlapping occurrence of `needle` in `haystack` with `with`.
fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// The longest form of a secret `mask` looks for (percent-encoding triples, Debug escapes
/// grow it further).
fn mask_margin(secrets: &[String]) -> usize {
    secrets.iter().map(|s| s.len()).max().unwrap_or(0) * 12
}

fn masked_body(body: &ReportBody, secrets: &[String]) -> (ReportBody, bool) {
    let margin = mask_margin(secrets);
    let window = EXCHANGE_BODY_LIMIT.saturating_add(margin);
    match body {
        ReportBody::Utf8(text) => {
            let mut end = window.min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let masked = mask(&text[..end], secrets);
            let cut = masked.len() > EXCHANGE_BODY_LIMIT || end < text.len();
            let mut end = EXCHANGE_BODY_LIMIT.min(masked.len());
            while !masked.is_char_boundary(end) {
                end -= 1;
            }
            (ReportBody::Utf8(masked[..end].to_owned()), cut)
        }
        ReportBody::Base64(encoded) => {
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
                return (ReportBody::Base64(String::new()), true);
            };
            let window = &bytes[..window.min(bytes.len())];
            let mut masked = window.to_vec();
            for secret in secrets {
                masked = replace_bytes(&masked, secret.as_bytes(), MASK.as_bytes());
            }
            let cut = masked.len() > EXCHANGE_BODY_LIMIT || window.len() < bytes.len();
            masked.truncate(EXCHANGE_BODY_LIMIT);
            (
                ReportBody::Base64(base64::engine::general_purpose::STANDARD.encode(masked)),
                cut,
            )
        }
    }
}

/// Builds the masked [`Exchange`] of a finished send (`send_with` does this itself; shells
/// that send without it, such as `rt agent`, use it with the values they know). `secrets` is the value list of the
/// send (longest first, as `send_with` keeps it). The response body is masked before it is
/// cut, so a secret that straddles the limit leaves no prefix behind.
pub fn exchange_of(
    report: &SendReport,
    secrets: &[String],
    caller: Caller,
    collection: Option<String>,
) -> Exchange {
    // Clone everything but the body, which can be large and is built separately.
    let mut shell = SendReport {
        response: report.response.as_ref().map(|r| ReceivedResponse {
            body: ReportBody::Utf8(String::new()),
            ..r.clone()
        }),
        ..report.clone()
    };
    let mut truncated = false;
    if let (Some(out), Some(src)) = (shell.response.as_mut(), report.response.as_ref()) {
        let (body, cut) = masked_body(&src.body, secrets);
        out.body = body;
        truncated = cut || src.truncated;
        mask_headers(&mut out.headers, secrets);
        out.url = mask(&out.url, secrets);
        for r in &mut out.redirects {
            r.location = mask(&r.location, secrets);
        }
    }
    if let Some(request) = shell.request.as_mut() {
        request.url = mask(&request.url, secrets);
        mask_headers(&mut request.headers, secrets);
    }
    Exchange {
        caller,
        collection,
        request_path: Some(report.request_path.clone()),
        environment: report.environment.clone(),
        at: now_ms(),
        report: shell,
        truncated,
    }
}

/// The secret values a shell that only sees the final request can recognize: values of
/// headers and query parameters whose name is secret (see [`secret_header_name`]), the token
/// after a scheme (`Bearer abc` yields `abc` too), and the cookie values of a `Cookie`
/// header. Longest first, like the list `send_with` keeps.
pub fn guess_secrets(url: &str, headers: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |v: &str| {
        let v = v.trim();
        if v.len() >= 3 && !out.iter().any(|o| o == v) {
            out.push(v.to_owned());
        }
    };
    for (name, value) in headers {
        if !secret_header_name(name) {
            continue;
        }
        add(value);
        if name.eq_ignore_ascii_case("cookie") {
            for pair in value.split(';') {
                if let Some((_, v)) = pair.split_once('=') {
                    add(v);
                }
            }
        } else if let Some((_, rest)) = value.split_once(' ') {
            add(rest);
        }
    }
    if let Ok(parsed) = url::Url::parse(url) {
        for (name, value) in parsed.query_pairs() {
            if secret_header_name(&name) {
                add(&value);
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.len()));
    out
}

fn now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// What the desktop builder manages as Tauri state (`builder().manage(ObserverHandle)`), the
/// same shape the CLI takes in `Distribution.observers`.
pub type ObserverHandle = ObserverSet;

impl std::fmt::Debug for ObserverSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ObserverSet({})", self.0.len())
    }
}

impl PartialEq for ObserverSet {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().zip(&other.0).all(|(a, b)| Arc::ptr_eq(a, b))
    }
}

impl Eq for ObserverSet {}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Count(Mutex<usize>);

    impl ExchangeObserver for Count {
        fn observe(&self, _: &Exchange) {
            *self.0.lock().unwrap() += 1;
        }
    }

    fn exchange() -> Exchange {
        Exchange {
            caller: Caller::Cli,
            collection: None,
            request_path: Some("a.yml".into()),
            environment: None,
            at: 0,
            report: crate::send::SendReport {
                version: crate::send::REPORT_VERSION,
                request_path: "a.yml".into(),
                environment: None,
                request: None,
                response: None,
                variables: Vec::new(),
                unresolved: Vec::new(),
                error: None,
                notes: Vec::new(),
                tests: Vec::new(),
                assertions: Vec::new(),
                logs: Vec::new(),
                var_changes: Vec::new(),
                flow: None,
            },
            truncated: false,
        }
    }

    #[test]
    fn notifies_every_observer_in_order() {
        let a = Arc::new(Count(Mutex::new(0)));
        let b = Arc::new(Count(Mutex::new(0)));
        let set = ObserverSet::new().with(a.clone()).with(b.clone());
        assert_eq!(set.len(), 2);
        set.notify(&exchange());
        assert_eq!((*a.0.lock().unwrap(), *b.0.lock().unwrap()), (1, 1));
        assert!(ObserverSet::new().is_empty());
    }
}
