// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `prepare` of the landscape: the SAP query parameters `sap-client` and `sap-language`
//! from the variables `sapClient` and `sapLanguage`, and the two header rules that make one
//! collection work against the Business Accelerator Hub sandbox and the own systems alike.
//!
//! When it applies: the URL is an SAP Gateway service (`/sap/opu/odata/…` or
//! `/sap/opu/odata4/…`, see [`crate::service::split`]), or the variables in scope set
//! `sapClient` or `sapSandbox=true` (then any URL: the environment says it is an SAP
//! system). Otherwise nothing happens.
//!
//! - `sap-client=<sapClient>` and `sap-language=<sapLanguage>` are appended when the variable
//!   is set and the URL has no parameter of that name (any case). Never duplicated, never
//!   replaced: a parameter written into the request wins.
//! - The request the host passes is resolved and masked: a value that still contains `{{`
//!   or the mask `***` is not copied, and a URL that contains the mask (a secret in the
//!   query) is not patched at all, since that would send the mask (a note says so).
//! - `APIKey` header (the Hub sandbox key, set collection-wide by the `sap-hub` importer):
//!   removed when it is empty or unresolved (`{{apiKey}}` in DEV/QAS/PRD, which have no key).
//! - Variable `sapSandbox=true` (the Hub sandbox environment): the `Authorization` header is
//!   removed, because the sandbox authenticates by `APIKey` only and the collection's Basic
//!   auth is meant for the own systems.

use rt_plugin_sdk::api::{PluginContext, PluginNote, PluginRequest, PrepareReply, RequestPatch};
use rt_plugin_sdk::{Result, info, warning};

use crate::service;

/// The variables this module reads.
pub const SAP_CLIENT: &str = "sapClient";
pub const SAP_LANGUAGE: &str = "sapLanguage";
pub const SAP_SANDBOX: &str = "sapSandbox";
/// The Hub sandbox's key header.
pub const API_KEY_HEADER: &str = "APIKey";

pub fn prepare(request: &PluginRequest, context: &PluginContext) -> Result<PrepareReply> {
    let var = |name: &str| {
        context
            .variables
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    };
    let client = var(SAP_CLIENT);
    let sandbox = var(SAP_SANDBOX).is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if !is_sap_service(&request.url) && client.is_none() && !sandbox {
        return Ok(PrepareReply::default());
    }

    let mut patch = RequestPatch::default();
    let mut notes: Vec<PluginNote> = Vec::new();

    let mut url = request.url.clone();
    let mut added = Vec::new();
    for (param, variable, value) in [
        ("sap-client", SAP_CLIENT, client),
        ("sap-language", SAP_LANGUAGE, var(SAP_LANGUAGE)),
    ] {
        let Some(value) = value else { continue };
        if has_param(&url, param) {
            continue;
        }
        if value.contains("{{") || value.contains("***") {
            notes.push(warning(format!(
                "{param} not added: the variable {variable} is not resolved"
            )));
            continue;
        }
        url = add_param(&url, param, value);
        added.push(format!("{param}={}", encode(value)));
    }
    if !added.is_empty() && request.url.contains("***") {
        // The URL carries a masked secret; a patched URL would send the mask (and the host
        // refuses patches with `***`), so the parameters cannot be added.
        notes.push(warning(format!(
            "SAP: {} not added: the URL contains a secret value; write the parameters into the request",
            added.join(", ")
        )));
    } else if !added.is_empty() {
        let env = context
            .environment
            .as_deref()
            .map(|e| format!(" (environment {e})"))
            .unwrap_or_default();
        notes.push(info(format!("SAP: added {}{env}", added.join(", "))));
        patch.url = Some(url);
    }

    for (name, value) in &request.headers {
        if name.eq_ignore_ascii_case(API_KEY_HEADER)
            && (value.trim().is_empty() || value.contains("{{"))
        {
            patch.remove_headers.push(name.clone());
            notes.push(info(format!(
                "SAP: header {name} removed (no API key in this environment)"
            )));
        }
    }
    if sandbox {
        for (name, _) in &request.headers {
            if name.eq_ignore_ascii_case("authorization") {
                patch.remove_headers.push(name.clone());
                notes.push(info(
                    "SAP: Authorization removed for the API Hub sandbox (it uses the APIKey header)",
                ));
            }
        }
    }
    Ok(PrepareReply { patch, notes })
}

/// An SAP Gateway URL: `/sap/opu/odata/` (V2) or `/sap/opu/odata4/` (V4).
pub fn is_sap_service(url: &str) -> bool {
    let Some(target) = service::split(url, &[]) else {
        return false;
    };
    target.version.is_some() && {
        let root = target.service_root.to_ascii_lowercase();
        root.contains("/sap/opu/odata/") || root.contains("/sap/opu/odata4/")
    }
}

/// Whether the query of `url` has a parameter `name` (ASCII case-insensitive, also when
/// percent-encoded).
pub fn has_param(url: &str, name: &str) -> bool {
    let before_fragment = url.split('#').next().unwrap_or(url);
    let Some((_, query)) = before_fragment.split_once('?') else {
        return false;
    };
    query.split('&').any(|pair| {
        let key = pair.split('=').next().unwrap_or(pair);
        decode(key).eq_ignore_ascii_case(name)
    })
}

/// `url` with `name=value` appended to the query, before a fragment.
pub fn add_param(url: &str, name: &str, value: &str) -> String {
    let (base, fragment) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let separator = match base.find('?') {
        None => "?",
        Some(i) if i + 1 == base.len() || base.ends_with('&') => "",
        Some(_) => "&",
    };
    format!("{base}{separator}{name}={}{fragment}", encode(value))
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_in_the_query() {
        assert!(has_param("http://h/s?sap-client=100", "sap-client"));
        assert!(has_param("http://h/s?a=1&SAP-Client=100", "sap-client"));
        assert!(has_param("http://h/s?sap%2Dclient=100", "sap-client"));
        assert!(!has_param("http://h/s?xsap-client=1", "sap-client"));
        assert!(!has_param("http://h/s#?sap-client=1", "sap-client"));
        assert_eq!(add_param("http://h/s", "a", "1"), "http://h/s?a=1");
        assert_eq!(add_param("http://h/s?", "a", "1"), "http://h/s?a=1");
        assert_eq!(
            add_param("http://h/s?x=1#f", "a", "b c"),
            "http://h/s?x=1&a=b%20c#f"
        );
    }

    #[test]
    fn sap_urls() {
        assert!(is_sap_service(
            "https://h/sap/opu/odata/sap/ZRT_SALES_SRV/SalesOrderSet"
        ));
        assert!(is_sap_service(
            "https://h/sap/opu/odata4/sap/z/srvd_a2x/sap/z/0001/SalesOrder"
        ));
        assert!(!is_sap_service(
            "http://localhost:4004/odata/v4/catalog/Books"
        ));
        assert!(!is_sap_service("https://api.example.com/users"));
    }
}
