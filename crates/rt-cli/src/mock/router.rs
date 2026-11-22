// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Routes of `rt mock`: which saved example answers a request. Pure, no I/O.

use rt_core::model::{Body, Item, ParamKind};
use rt_core::prepare::{PrepareInput, scope};
use rt_core::vars::Scope;
use rt_format::CollectionDoc;
use rt_format::examples::{Example, ExampleBody};
use serde::Serialize;

/// The incoming request, as the server hands it over.
#[derive(Debug, Clone, Default)]
pub struct MockRequest<'a> {
    pub method: &'a str,
    /// The path, still percent-encoded, without the query.
    pub path: &'a str,
    /// The raw query string without `?` (may be empty).
    pub query: &'a str,
    pub headers: Vec<(String, String)>,
    pub body: &'a [u8],
}

/// What the mock answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    /// `statusText` of the example, if any.
    pub reason: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// `(request path, example name)` of the example that answered; `None`: 404.
    pub matched: Option<(String, String)>,
}

/// One route of the mock, for the banner and the 404 answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteInfo {
    pub method: String,
    /// The path template, e.g. `/users/:id`.
    pub path: String,
    /// The request file.
    pub request: String,
    /// Names of the examples on this route, in file order.
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    /// `:id`, `{id}` or a segment with a `{{variable}}`; the text as written.
    Any(String),
}

/// A condition from the example's saved request; met, missed or not applicable.
#[derive(Debug, Clone, PartialEq)]
enum Condition {
    /// The incoming path segment at this index equals the value.
    Segment(usize, String),
    Query(String, String),
    /// Header name in lower case.
    Header(String, String),
    Body(String),
}

#[derive(Debug, Clone)]
struct Entry {
    method: String,
    template: Vec<Segment>,
    request: String,
    example: String,
    conditions: Vec<Condition>,
    status: u16,
    reason: Option<String>,
    headers: Vec<(String, String)>,
    body: Option<ExampleBody>,
}

/// The routes of one collection.
#[derive(Debug, Clone, Default)]
pub struct Mock {
    entries: Vec<Entry>,
}

impl Mock {
    /// Routes for every HTTP request of `doc` with saved examples. Variables are resolved
    /// with the collection, folders, the request and, with `environment`, that environment.
    pub fn new(doc: &CollectionDoc, environment: Option<&str>) -> Result<Self, String> {
        let model = doc.model();
        if let Some(env) = environment
            && model.environment(env).is_none()
        {
            let known: Vec<&str> = model.environments.iter().map(|e| e.name.as_str()).collect();
            return Err(format!(
                "no environment {env:?} in the collection (known: {})",
                if known.is_empty() {
                    "none".to_owned()
                } else {
                    known.join(", ")
                }
            ));
        }
        let input = PrepareInput {
            environment,
            ..PrepareInput::default()
        };
        let mut entries = Vec::new();
        // Model order (by seq, then name), so ties go to what the app shows first.
        let mut paths = Vec::new();
        request_paths(&model.items, &mut paths);
        let all = doc.examples();
        for path in paths {
            let Some(saved) = all.iter().find(|r| r.path == path) else {
                continue;
            };
            let Some(context) = model.request(&path) else {
                continue;
            };
            if !context.request.spec.protocol.is_http() {
                continue;
            }
            let scope = scope(&context, &input).map_err(|e| e.to_string())?;
            let spec = &context.request.spec;
            for example in &saved.examples {
                entries.push(entry(&scope, &path, &spec.method, &spec.url, example));
            }
        }
        Ok(Self { entries })
    }

    /// The routes, one per method, path and request.
    pub fn routes(&self) -> Vec<RouteInfo> {
        let mut out: Vec<RouteInfo> = Vec::new();
        for e in &self.entries {
            let path = display(&e.template);
            match out
                .iter_mut()
                .find(|r| r.method == e.method && r.path == path && r.request == e.request)
            {
                Some(r) => r.examples.push(e.example.clone()),
                None => out.push(RouteInfo {
                    method: e.method.clone(),
                    path,
                    request: e.request.clone(),
                    examples: vec![e.example.clone()],
                }),
            }
        }
        out
    }

    /// The answer to `req`: the best fitting example, or 404 with the routes.
    pub fn answer(&self, req: &MockRequest<'_>) -> Answer {
        let segments = split_path(req.path, true);
        let query = parse_query(req.query);
        let mut best: Option<(&Entry, (usize, usize))> = None;
        for e in &self.entries {
            if !e.method.eq_ignore_ascii_case(req.method) || !fits(&e.template, &segments) {
                continue;
            }
            let score = score(e, &segments, &query, req);
            // Most conditions met, then fewest missed; the first entry wins ties.
            let better = match &best {
                None => true,
                Some((_, (met, missed))) => {
                    score.0 > *met || (score.0 == *met && score.1 < *missed)
                }
            };
            if better {
                best = Some((e, score));
            }
        }
        match best {
            Some((e, _)) => respond(e),
            None => self.not_found(req),
        }
    }

    fn not_found(&self, req: &MockRequest<'_>) -> Answer {
        let path = if req.query.is_empty() {
            req.path.to_owned()
        } else {
            format!("{}?{}", req.path, req.query)
        };
        let body = serde_json::json!({
            "error": format!("no saved example matches {} {path}", req.method),
            "routes": self.routes(),
        });
        Answer {
            status: 404,
            reason: None,
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec_pretty(&body).unwrap_or_default(),
            matched: None,
        }
    }
}

fn request_paths(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Request(r) => out.push(r.path.clone()),
            Item::Folder(f) => request_paths(&f.items, out),
        }
    }
}

fn interpolate(scope: &Scope, text: &str) -> String {
    if text.contains("{{") {
        scope.interpolate(text).value
    } else {
        text.to_owned()
    }
}

/// A value that still has a `{{placeholder}}` cannot be compared.
fn concrete(value: &str) -> bool {
    !value.contains("{{")
}

fn entry(scope: &Scope, path: &str, method: &str, url: &str, example: &Example) -> Entry {
    let saved = &example.request;
    let method = saved
        .method
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(method)
        .trim()
        .to_ascii_uppercase();
    let template = template(&path_of(&interpolate(scope, url)));
    let mut conditions = Vec::new();
    let mut own_template = None;
    if let Some(saved_url) = &saved.url {
        let saved_url = interpolate(scope, saved_url);
        let saved_template = template_of(&saved_url);
        if fits_template(&template, &saved_template) {
            // Literal segments where the request has a wildcard: `/users/42` for `/users/:id`.
            for (i, (t, s)) in template.iter().zip(&saved_template).enumerate() {
                if let (Segment::Any(_), Segment::Literal(v)) = (t, s) {
                    conditions.push(Condition::Segment(i, v.clone()));
                }
            }
        } else {
            // Saved against another path: that path is the route.
            own_template = Some(saved_template);
        }
        if let Some((_, q)) = saved_url.split_once('?') {
            for (k, v) in parse_query(q.split('#').next().unwrap_or("")) {
                if concrete(&k) && concrete(&v) {
                    conditions.push(Condition::Query(k, v));
                }
            }
        }
    }
    let template = own_template.unwrap_or(template);
    for p in saved.params.iter().filter(|p| p.enabled) {
        let value = interpolate(scope, &p.value);
        if !concrete(&value) {
            continue;
        }
        match p.kind {
            ParamKind::Query => {
                let c = Condition::Query(p.name.clone(), value);
                if !conditions.contains(&c) {
                    conditions.push(c);
                }
            }
            ParamKind::Path => {
                let at = template.iter().position(|s| match s {
                    Segment::Any(text) => wildcard_name(text) == Some(p.name.as_str()),
                    Segment::Literal(_) => false,
                });
                if let Some(i) = at {
                    conditions.retain(|c| !matches!(c, Condition::Segment(j, _) if *j == i));
                    conditions.push(Condition::Segment(i, value));
                }
            }
        }
    }
    for h in saved.headers.iter().filter(|h| h.enabled) {
        let value = interpolate(scope, &h.value);
        if concrete(&value) && !h.name.is_empty() {
            conditions.push(Condition::Header(h.name.to_ascii_lowercase(), value));
        }
    }
    match &saved.body {
        Body::Raw { data, .. } | Body::GraphQl { query: data, .. } => {
            let data = interpolate(scope, data);
            if concrete(&data) && !data.trim().is_empty() {
                conditions.push(Condition::Body(data));
            }
        }
        Body::FormUrlEncoded { fields } => {
            let pairs: Vec<String> = fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| format!("{}={}", f.name, interpolate(scope, &f.value)))
                .collect();
            let data = pairs.join("&");
            if concrete(&data) && !data.is_empty() {
                conditions.push(Condition::Body(data));
            }
        }
        _ => {}
    }
    Entry {
        method,
        template,
        request: path.to_owned(),
        example: example.name.clone(),
        conditions,
        status: example.response.status,
        reason: example.response.status_text.clone(),
        headers: example.response.headers.clone(),
        body: example.response.body.clone(),
    }
}

/// `:id` → `id`, `{id}` → `id`, `{{id}}` → `id`.
fn wildcard_name(text: &str) -> Option<&str> {
    if let Some(name) = text.strip_prefix(':') {
        return Some(name);
    }
    let inner = text
        .strip_prefix("{{")
        .and_then(|t| t.strip_suffix("}}"))
        .or_else(|| text.strip_prefix('{').and_then(|t| t.strip_suffix('}')))?;
    Some(inner)
}

/// The path of a URL template: without scheme and host, a leading `{{variable}}` (the base
/// URL), the query and the fragment.
pub(crate) fn path_of(url: &str) -> String {
    let url = url.trim();
    let url = url.split(['?', '#']).next().unwrap_or("");
    let rest = if let Some(i) = url.find("://") {
        let after = &url[i + 3..];
        return after
            .find('/')
            .map_or_else(|| "/".to_owned(), |j| after[j..].to_owned());
    } else if url.starts_with("{{")
        && let Some(end) = url.find("}}")
    {
        let after = &url[end + 2..];
        if after.starts_with(':') {
            // `{{host}}:8080/path`
            after.find('/').map_or("", |j| &after[j..])
        } else {
            after
        }
    } else if url.starts_with('/') {
        url
    } else {
        // `localhost:3000/users`
        url.find('/').map_or("", |j| &url[j..])
    };
    if rest.starts_with('/') {
        rest.to_owned()
    } else {
        format!("/{rest}")
    }
}

fn template(path: &str) -> Vec<Segment> {
    split_path(path, false)
        .into_iter()
        .map(|s| {
            if s.starts_with(':')
                || s.contains("{{")
                || (s.starts_with('{') && s.ends_with('}') && s.len() > 2)
            {
                Segment::Any(s)
            } else {
                Segment::Literal(s)
            }
        })
        .collect()
}

fn template_of(url: &str) -> Vec<Segment> {
    template(&path_of(url))
}

/// Whether a saved example URL has the shape of the request's template.
fn fits_template(request: &[Segment], saved: &[Segment]) -> bool {
    request.len() == saved.len()
        && request.iter().zip(saved).all(|(r, s)| match (r, s) {
            (Segment::Literal(a), Segment::Literal(b)) => a == b,
            _ => true,
        })
}

fn fits(template: &[Segment], segments: &[String]) -> bool {
    template.len() == segments.len()
        && template.iter().zip(segments).all(|(t, s)| match t {
            Segment::Literal(l) => l == s,
            Segment::Any(_) => true,
        })
}

fn display(template: &[Segment]) -> String {
    let parts: Vec<&str> = template
        .iter()
        .map(|s| match s {
            Segment::Literal(t) | Segment::Any(t) => t.as_str(),
        })
        .collect();
    format!("/{}", parts.join("/"))
}

/// Non-empty segments; `decode` percent-decodes them (incoming paths).
fn split_path(path: &str, decode: bool) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if decode {
                percent_decode(s, false)
            } else {
                s.to_owned()
            }
        })
        .collect()
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k, true), percent_decode(v, true))
        })
        .collect()
}

fn percent_decode(s: &str, plus_is_space: bool) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' if plus_is_space => {
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

/// `(met, missed)` conditions of `e` for this request.
fn score(
    e: &Entry,
    segments: &[String],
    query: &[(String, String)],
    req: &MockRequest<'_>,
) -> (usize, usize) {
    let (mut met, mut missed) = (0, 0);
    for c in &e.conditions {
        let ok = match c {
            Condition::Segment(i, v) => segments.get(*i) == Some(v),
            Condition::Query(k, v) => query.iter().any(|(qk, qv)| qk == k && qv == v),
            Condition::Header(name, v) => req
                .headers
                .iter()
                .any(|(n, hv)| n.eq_ignore_ascii_case(name) && hv.trim() == v.trim()),
            Condition::Body(expected) => same_body(expected, req.body),
        };
        if ok {
            met += 1;
        } else {
            missed += 1;
        }
    }
    (met, missed)
}

/// JSON compared as JSON, anything else as trimmed text.
fn same_body(expected: &str, actual: &[u8]) -> bool {
    let Ok(actual) = std::str::from_utf8(actual) else {
        return false;
    };
    match (
        serde_json::from_str::<serde_json::Value>(expected),
        serde_json::from_str::<serde_json::Value>(actual),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => expected.trim() == actual.trim(),
    }
}

fn content_type(kind: &str) -> &'static str {
    match kind {
        "json" => "application/json",
        "xml" => "application/xml",
        "html" => "text/html; charset=utf-8",
        "binary" => "application/octet-stream",
        _ => "text/plain; charset=utf-8",
    }
}

fn respond(e: &Entry) -> Answer {
    let mut headers: Vec<(String, String)> = e
        .headers
        .iter()
        .filter(|(n, _)| {
            // The server frames the body itself.
            !n.eq_ignore_ascii_case("content-length")
                && !n.eq_ignore_ascii_case("transfer-encoding")
                && !n.eq_ignore_ascii_case("content-encoding")
                && !n.eq_ignore_ascii_case("connection")
        })
        .cloned()
        .collect();
    if let Some(body) = &e.body
        && !headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("content-type".into(), content_type(&body.kind).into()));
    }
    Answer {
        status: e.status,
        reason: e.reason.clone(),
        headers,
        body: e
            .body
            .as_ref()
            .map(|b| b.data.as_bytes().to_vec())
            .unwrap_or_default(),
        matched: Some((e.request.clone(), e.example.clone())),
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn paths_of_url_templates() {
        assert_eq!(path_of("{{baseUrl}}/users/:id?x=1"), "/users/:id");
        assert_eq!(path_of("https://api.example.com/v1/users"), "/v1/users");
        assert_eq!(path_of("http://localhost:3000"), "/");
        assert_eq!(path_of("{{host}}:8080/a/b#frag"), "/a/b");
        assert_eq!(path_of("localhost:3000/users"), "/users");
        assert_eq!(path_of("/health"), "/health");
        assert_eq!(path_of("{{baseUrl}}"), "/");
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b", false), "a b");
        assert_eq!(percent_decode("a+b", true), "a b");
        assert_eq!(percent_decode("a+b", false), "a+b");
        assert_eq!(percent_decode("100%", false), "100%");
        assert_eq!(percent_decode("%4", false), "%4");
        assert_eq!(percent_decode("%41", false), "A");
        assert_eq!(percent_decode("%zz", false), "%zz");
    }
}
