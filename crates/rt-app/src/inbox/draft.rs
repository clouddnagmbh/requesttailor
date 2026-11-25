// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! "Make request": a captured request as a request that replays it against `{{baseUrl}}`.
//! Runs everywhere (the web shell calls it through rt-wasm).
//!
//! What is kept: method, path, query, the headers that make sense to replay and the body as
//! text. What is not: `Host` and the hop-by-hop headers (they belong to the connection), the
//! `Content-Length` (computed when sending), `Accept-Encoding`, and a body that was not text.
//!
//! The secret rule: `Authorization`, `Proxy-Authorization`, `Cookie` and every header, query
//! parameter, form field or JSON key whose name contains `signature`, `token`, `secret`, `key`
//! or `password` become a variable `{{<name>}}` (listed in [`InboxDraft::secrets`]) **without
//! a value**: the user sets it in an environment, where it goes to the key store. The secret
//! scanner is the second net over header values and the body; what it finds is replaced the
//! same way. The result carries no value of the capture that looks like a credential.

use rt_core::model::{Body, KeyValue, Param, ParamKind, RawKind, RequestSpec};

use super::InboxDraft;
use crate::secret_scan;
use rt_core::inbox::CapturedRequest;

/// The variable that stands for the base URL of the service.
pub const BASE_URL: &str = "baseUrl";

const DROPPED: [&str; 11] = [
    "host",
    "connection",
    "keep-alive",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "content-length",
    "expect",
    "accept-encoding",
];

const SENSITIVE_PARTS: [&str; 6] = ["signature", "token", "secret", "key", "password", "passwd"];
const SENSITIVE_NAMES: [&str; 3] = ["authorization", "proxy-authorization", "cookie"];

fn sensitive(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    SENSITIVE_NAMES.contains(&lower.as_str()) || SENSITIVE_PARTS.iter().any(|p| lower.contains(p))
}

/// `X-Hub-Signature-256` -> `x_hub_signature_256`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_owned();
    if out.is_empty() {
        "secret".to_owned()
    } else {
        out
    }
}

#[derive(Default)]
struct Secrets(Vec<String>);

impl Secrets {
    /// `{{slug}}`, noting the name.
    fn variable(&mut self, name: &str) -> String {
        let s = slug(name);
        if !self.0.contains(&s) {
            self.0.push(s.clone());
        }
        format!("{{{{{s}}}}}")
    }

    /// Replaces what the secret scanner finds in `text` (a line as `name: value` or a body).
    fn scrub(&mut self, text: &str, label: &str) -> String {
        let mut out = text.to_owned();
        for (finding, values) in secret_scan::scan_values("inbox.yml", text) {
            for value in values {
                if value.len() >= 4 && out.contains(&value) {
                    let var = self.variable(&format!("{label} {}", finding.kind));
                    out = out.replace(&value, &var);
                }
            }
        }
        out
    }
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The draft for one captured request.
pub fn draft(request: &CapturedRequest) -> InboxDraft {
    let mut secrets = Secrets::default();
    let (path, query) = match request.target.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (request.target.as_str(), None),
    };

    let mut params = Vec::new();
    let mut url = format!("{{{{{BASE_URL}}}}}{path}");
    if let Some(query) = query {
        let mut pairs = Vec::new();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = if sensitive(&percent_decode(name)) && !value.is_empty() {
                secrets.variable(&percent_decode(name))
            } else {
                value.to_owned()
            };
            pairs.push(format!("{name}={value}"));
            params.push(Param {
                name: percent_decode(name),
                value,
                kind: ParamKind::Query,
                enabled: true,
            });
        }
        if !pairs.is_empty() {
            url.push('?');
            url.push_str(&pairs.join("&"));
        }
    }

    let mut headers = Vec::new();
    for (name, value) in &request.headers {
        if DROPPED.iter().any(|d| name.eq_ignore_ascii_case(d)) {
            continue;
        }
        let value = if sensitive(name) {
            secrets.variable(name)
        } else {
            let line = secrets.scrub(&format!("{name}: {value}\n"), name);
            line.strip_prefix(&format!("{name}: "))
                .map_or_else(|| value.clone(), |v| v.trim_end_matches('\n').to_owned())
        };
        headers.push(KeyValue::new(name.clone(), value));
    }

    let content_type = header_value(&request.headers, "content-type")
        .unwrap_or("")
        .to_ascii_lowercase();
    let body = match request.body.as_deref() {
        None | Some("") => Body::None,
        Some(text) if content_type.contains("x-www-form-urlencoded") => {
            let fields = text
                .split('&')
                .filter(|p| !p.is_empty())
                .map(|pair| {
                    let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                    let value = if sensitive(&percent_decode(name)) && !value.is_empty() {
                        secrets.variable(&percent_decode(name))
                    } else {
                        percent_decode(value)
                    };
                    KeyValue::new(percent_decode(name), value)
                })
                .collect();
            Body::FormUrlEncoded { fields }
        }
        Some(text) => {
            let kind = if content_type.contains("json") {
                RawKind::Json
            } else if content_type.contains("xml") {
                RawKind::Xml
            } else {
                RawKind::Text
            };
            let text = scrub_json_keys(text, &mut secrets);
            let text = secrets.scrub(&text, "body");
            Body::Raw { kind, data: text }
        }
    };

    let name = format!("{} {}", request.method, path);
    let spec = RequestSpec {
        name: name.clone(),
        method: request.method.clone(),
        url,
        headers,
        params,
        body,
        ..RequestSpec::default()
    };
    InboxDraft {
        name,
        spec,
        secrets: secrets.0,
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `"token": "x"` -> `"token": "{{token}}"` for every string value whose key names a credential;
/// the formatting of the rest stays.
fn scrub_json_keys(text: &str, secrets: &mut Secrets) -> String {
    if !text.contains('"') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let Some(end) = string_end(bytes, i) else {
            break;
        };
        let key = &text[i + 1..end];
        i = end + 1;
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if bytes.get(j) != Some(&b':') || !sensitive(key) {
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if bytes.get(j) != Some(&b'"') {
            continue;
        }
        let Some(vend) = string_end(bytes, j) else {
            break;
        };
        let value = &text[j + 1..vend];
        if !value.is_empty() && !(value.starts_with("{{") && value.ends_with("}}")) {
            out.push_str(&text[copied..j + 1]);
            out.push_str(&secrets.variable(key));
            copied = vend;
        }
        i = vend + 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// The index of the closing quote of the string starting at `start`.
fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}
