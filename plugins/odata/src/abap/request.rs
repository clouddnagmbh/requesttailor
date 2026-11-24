// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The request as the snippet needs it: variables resolved, secrets and session headers left
//! out, the URL split into the destination's host and the path the snippet sends to.

use rt_plugin_sdk::model::{Auth, Body, ParamKind, Protocol, RawKind, RequestSpec};

use crate::write::batch::{self, Item};

/// Why there is no snippet for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable(pub String);

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unavailable {}

fn no<T>(why: impl Into<String>) -> Result<T, Unavailable> {
    Err(Unavailable(why.into()))
}

/// What the call does; it decides the CSRF handshake and `If-Match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Read,
    /// A `$batch` of reads only.
    BatchRead,
    Create,
    Update,
    Delete,
}

impl Kind {
    pub fn needs_token(self) -> bool {
        matches!(
            self,
            Kind::Create | Kind::Update | Kind::Delete | Kind::BatchRead
        )
    }

    pub fn needs_etag(self) -> bool {
        matches!(self, Kind::Update | Kind::Delete)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub content_type: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    /// Upper case; `MERGE` and the like are kept.
    pub method: String,
    pub kind: Kind,
    /// `scheme://host[:port]` of the URL, `None` if the URL has no known host.
    pub origin: Option<String>,
    /// Service root path with leading and trailing `/` (for the token fetch).
    pub service_root: String,
    /// Path and query with a leading `/`.
    pub uri: String,
    /// Headers that are safe to copy, in order.
    pub headers: Vec<(String, String)>,
    /// A `If-Match` value written in the request.
    pub if_match: Option<String>,
    pub body: Option<Payload>,
    /// Names of headers and query parameters left out because they carry secrets or sessions.
    pub omitted: Vec<String>,
    /// `{{variables}}` that had no value; they stand as `<name>` in the snippet.
    pub unresolved: Vec<String>,
    /// The request has an auth block (not copied).
    pub had_auth: bool,
}

/// Headers the snippet never copies: logon data, sessions, and what the HTTP client sets.
fn skipped_header(name: &str) -> Option<bool> {
    let n = name.to_ascii_lowercase();
    match n.as_str() {
        "content-length" | "host" | "connection" | "accept-encoding" | "user-agent"
        | "content-type" | "if-match" => Some(false),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "x-csrf-token" => {
            Some(true)
        }
        _ if looks_secret(&n) => Some(true),
        _ => None,
    }
}

/// Names that hold secrets (headers, query parameters).
fn looks_secret(lower: &str) -> bool {
    ["key", "token", "secret", "password", "passwd", "credential"]
        .iter()
        .any(|w| lower.contains(w))
}

/// `{{name}}` replaced from `variables`; names without a value become `<name>` and are
/// collected.
fn resolve(template: &str, variables: &[(String, String)], unresolved: &mut Vec<String>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(end) = rest[start..].find("}}").map(|e| start + e) else {
            break;
        };
        out.push_str(&rest[..start]);
        let name = rest[start + 2..end].trim();
        match variables.iter().find(|(n, _)| n == name) {
            Some((_, v)) if v != "***" => out.push_str(v),
            _ => {
                out.push_str(&format!("<{name}>"));
                if !unresolved.iter().any(|u| u == name) {
                    unresolved.push(name.to_owned());
                }
            }
        }
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

pub fn build(spec: &RequestSpec, variables: &[(String, String)]) -> Result<Call, Unavailable> {
    if !matches!(spec.protocol, Protocol::Http) {
        return no("not available for this request: only HTTP requests can be copied");
    }
    let method = spec.method.trim().to_ascii_uppercase();
    if method.is_empty() || !method.chars().all(|c| c.is_ascii_alphabetic()) {
        return no("not available for this request: no HTTP method");
    }

    let mut unresolved = Vec::new();
    let mut omitted: Vec<String> = Vec::new();

    // URL: variables first, then path parameters.
    let mut url = resolve(&spec.url, variables, &mut unresolved);
    for p in spec
        .params
        .iter()
        .filter(|p| p.enabled && p.kind == ParamKind::Path)
    {
        let value = resolve(&p.value, variables, &mut unresolved);
        url = url.replace(&format!(":{}", p.name), &value);
    }
    let (base, query) = match url
        .split_once('#')
        .map_or(url.as_str(), |(b, _)| b)
        .split_once('?')
    {
        Some((b, q)) => (b.to_owned(), q.to_owned()),
        None => (
            url.split('#').next().unwrap_or_default().to_owned(),
            String::new(),
        ),
    };
    let (origin, path) = split_origin(&base);
    let query = clean_query(&query, &mut omitted);
    let mut uri = path;
    if !query.is_empty() {
        uri.push('?');
        uri.push_str(&query);
    }
    let service_root = {
        let full = format!(
            "{}{}",
            origin.clone().unwrap_or_else(|| "http://host".into()),
            uri
        );
        crate::service::split(&full, &[])
            .map(|t| t.service_root)
            .and_then(|r| r.split_once("://").map(|(_, rest)| rest.to_owned()))
            .and_then(|rest| rest.find('/').map(|i| rest[i..].to_owned()))
            .unwrap_or_else(|| "/".into())
    };

    // Headers.
    let mut headers = Vec::new();
    let mut content_type = None;
    let mut if_match = None;
    for h in spec.headers.iter().filter(|h| h.enabled) {
        match skipped_header(&h.name) {
            Some(true) => omitted.push(h.name.clone()),
            Some(false) => {
                let value = resolve(&h.value, variables, &mut unresolved);
                if h.name.eq_ignore_ascii_case("content-type") {
                    content_type = Some(value);
                } else if h.name.eq_ignore_ascii_case("if-match") {
                    if_match = Some(value);
                }
            }
            None => {
                let value = resolve(&h.value, variables, &mut unresolved);
                headers.push((h.name.clone(), value));
            }
        }
    }
    let had_auth = !matches!(spec.auth, Auth::Inherit | Auth::None);

    // Body.
    let (body, batch_items) = match &spec.body {
        Body::None => (None, None),
        Body::Raw { kind, data } => {
            let text = resolve(data, variables, &mut unresolved);
            let ct = content_type.clone().unwrap_or_else(|| {
                match kind {
                    RawKind::Json => "application/json",
                    RawKind::Xml => "application/xml",
                    RawKind::Text | RawKind::Sparql => "text/plain",
                }
                .to_owned()
            });
            let items =
                batch::parse_request(&ct, &text.replace("\r\n", "\n").replace('\n', "\r\n"));
            (
                (!text.trim().is_empty()).then_some(Payload {
                    content_type: ct,
                    text,
                }),
                items,
            )
        }
        _ => {
            return no(
                "not available for this request: only raw JSON, XML and text bodies can be copied",
            );
        }
    };

    let is_batch = {
        let p = uri.split('?').next().unwrap_or_default();
        p.rsplit('/').next() == Some("$batch")
    };
    let kind = if is_batch && method == "POST" {
        match batch_items {
            Some(items)
                if !items.is_empty()
                    && items.iter().all(
                        |i| matches!(i, Item::Read(p) if p.method.eq_ignore_ascii_case("GET")),
                    ) =>
            {
                Kind::BatchRead
            }
            Some(_) => {
                return no(
                    "not available for this request: a $batch with changes (a changeset or a write) is not copied; copy the single requests instead",
                );
            }
            None => {
                return no(
                    "not available for this request: the $batch body is not a multipart batch",
                );
            }
        }
    } else {
        match method.as_str() {
            "GET" | "HEAD" => Kind::Read,
            "POST" => Kind::Create,
            "PUT" | "PATCH" | "MERGE" => Kind::Update,
            "DELETE" => Kind::Delete,
            other => return no(format!("not available for this request: method {other}")),
        }
    };
    // A body on a read or a delete is dropped: the services ignore it.
    let body = match kind {
        Kind::Read | Kind::Delete => None,
        _ => body,
    };
    omitted.dedup();
    Ok(Call {
        name: spec.name.clone(),
        method,
        kind,
        origin,
        service_root,
        uri,
        headers,
        if_match,
        body,
        omitted,
        unresolved,
        had_auth,
    })
}

/// `https://host:port` and the path (with a leading `/`). A URL that starts with an unresolved
/// `<name>` stands for a base URL: it is dropped.
fn split_origin(base: &str) -> (Option<String>, String) {
    if let Some(i) = base.find("://") {
        let rest = &base[i + 3..];
        return match rest.find('/') {
            Some(j) => (Some(base[..i + 3 + j].to_owned()), rest[j..].to_owned()),
            None => (Some(base.to_owned()), "/".to_owned()),
        };
    }
    if base.starts_with('<')
        && let Some(end) = base.find('>')
    {
        let path = &base[end + 1..];
        return (
            None,
            if path.starts_with('/') {
                path.to_owned()
            } else {
                format!("/{path}")
            },
        );
    }
    (
        None,
        if base.starts_with('/') {
            base.to_owned()
        } else {
            format!("/{base}")
        },
    )
}

/// The query without values of secret-looking parameters (`name=<name>`).
fn clean_query(query: &str, omitted: &mut Vec<String>) -> String {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            if looks_secret(&name.to_ascii_lowercase()) && !value.starts_with('<') {
                omitted.push(format!("query parameter {name}"));
                format!("{name}=<{name}>")
            } else {
                pair.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}
