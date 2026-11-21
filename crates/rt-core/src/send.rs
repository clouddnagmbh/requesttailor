// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One entry point for "send this request": CLI, WASM and later the app all call this, so
//! their results are identical (product principle 5). `SendReport` is also the JSON that
//! `rt send` prints and that `rt-wasm` returns.
//!
//! Owner: wave 2, strand D.

use serde::{Deserialize, Serialize};

use crate::model::{Auth, Body, Collection, RequestContext, Variable};
use crate::prepare::{FileSource, PrepareError, PrepareInput, Prepared, prepare};
use crate::transport::{
    CompletedResponse, HttpRequest, HttpVersion, Redirect, Timing, Transport, TransportError,
    collect,
};
use crate::vars::VarUse;

/// Version of the `SendReport` JSON shape. Bump on breaking changes.
pub const REPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendOptions {
    /// Keep at most this many body bytes in the report.
    pub body_limit: usize,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            body_limit: 10 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentRequest {
    pub method: String,
    pub url: String,
    /// Secret values are masked as `***`.
    pub headers: Vec<(String, String)>,
    pub body_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding", content = "data", rename_all = "lowercase")]
pub enum ReportBody {
    /// Valid UTF-8.
    Utf8(String),
    /// Anything else, standard base64.
    Base64(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedResponse {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub redirects: Vec<Redirect>,
    pub body: ReportBody,
    pub size: u64,
    pub truncated: bool,
    pub timing: Timing,
    #[serde(with = "crate::serde_ms")]
    pub total: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum SendError {
    /// Request path not in the collection.
    NotFound(String),
    /// Variable, auth or body preparation failed.
    Prepare(String),
    Transport(TransportError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendReport {
    pub version: u32,
    /// Request path relative to the collection root.
    pub request_path: String,
    pub environment: Option<String>,
    /// What went on the wire; `None` if preparation failed.
    pub request: Option<SentRequest>,
    pub response: Option<ReceivedResponse>,
    pub variables: Vec<VarUse>,
    pub unresolved: Vec<String>,
    pub error: Option<SendError>,
}

/// What `rt send` prints in place of a secret value.
pub const MASK: &str = "***";

/// Finds the request, prepares it, sends it and reads the response.
///
/// Never fails: every outcome, including "request not found", is a [`SendReport`] with
/// `error` set. Secret values are masked in `report.request`; the response is reported as
/// received.
pub async fn send<T: Transport>(
    collection: &Collection,
    request_path: &str,
    input: &PrepareInput<'_>,
    files: &dyn FileSource,
    transport: &T,
    options: &SendOptions,
) -> SendReport {
    let report = SendReport {
        version: REPORT_VERSION,
        request_path: request_path.to_owned(),
        environment: input.environment.map(str::to_owned),
        request: None,
        response: None,
        variables: Vec::new(),
        unresolved: Vec::new(),
        error: None,
    };
    let Some(context) = collection.request(request_path) else {
        return SendReport {
            error: Some(SendError::NotFound(request_path.to_owned())),
            ..report
        };
    };
    let prepared = prepare(&context, input, files);
    finish(report, &context, input, prepared, transport, options).await
}

/// Everything after `prepare`. Split out so the orchestration can be tested with a
/// hand-built `Prepared`.
async fn finish<T: Transport>(
    mut report: SendReport,
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    prepared: Result<Prepared, PrepareError>,
    transport: &T,
    options: &SendOptions,
) -> SendReport {
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            report.unresolved = unresolved_fallback(context, input);
            report.error = Some(SendError::Prepare(e.to_string()));
            return report;
        }
    };
    let secrets = secret_values(context, input, &prepared);
    report.request = Some(sent_request(&prepared.request, &secrets));
    report.variables = prepared.used;
    report.unresolved = prepared.unresolved;

    let started = web_time::Instant::now();
    let result = match transport.send(prepared.request).await {
        Ok(response) => collect(response, options.body_limit, started).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(completed) => report.response = Some(received(completed)),
        Err(e) => report.error = Some(SendError::Transport(e)),
    }
    report
}

/// The request as it goes on the wire, with secrets masked. The body's content type is listed
/// as a header when the request does not set one, because the transport adds it.
fn sent_request(request: &HttpRequest, secrets: &[String]) -> SentRequest {
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(name, value)| (name.clone(), mask_header(value, secrets)))
        .collect();
    if let Some(body) = &request.body
        && let Some(content_type) = &body.content_type
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".to_owned(), content_type.clone()));
    }
    SentRequest {
        method: request.method.clone(),
        url: mask(&request.url, secrets),
        headers,
        body_size: request.body.as_ref().map_or(0, |b| b.bytes.len() as u64),
    }
}

fn received(c: CompletedResponse) -> ReceivedResponse {
    ReceivedResponse {
        status: c.head.status,
        version: c.head.version,
        headers: c.head.headers,
        url: c.head.url,
        redirects: c.head.redirects,
        body: report_body(&c.body, c.truncated),
        size: c.size,
        truncated: c.truncated,
        timing: c.head.timing,
        total: c.total,
    }
}

/// UTF-8 bodies as text, anything else as base64. A truncated body that is valid UTF-8 except
/// for a character cut at the limit is reported as text without that partial character.
fn report_body(bytes: &[u8], truncated: bool) -> ReportBody {
    match std::str::from_utf8(bytes) {
        Ok(s) => ReportBody::Utf8(s.to_owned()),
        Err(e) if truncated && e.error_len().is_none() => {
            ReportBody::Utf8(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned())
        }
        Err(_) => ReportBody::Base64(base64_encode(bytes)),
    }
}

// ---------------------------------------------------------------------------------------------
// Secret masking

/// Values that must not appear in the report: every value of a variable that is secret in any
/// layer, declared secret anywhere in the collection, or reported secret by `prepare`.
/// Longest first, so a secret that contains another one is masked as a whole.
fn secret_values(
    context: &RequestContext<'_>,
    input: &PrepareInput<'_>,
    prepared: &Prepared,
) -> Vec<String> {
    let mut names: Vec<&str> = declared_vars(context, input)
        .filter(|v| v.secret)
        .map(|v| v.name.as_str())
        .collect();
    names.extend(
        prepared
            .used
            .iter()
            .filter(|u| u.secret)
            .map(|u| u.name.as_str()),
    );
    let mut values: Vec<String> = Vec::new();
    for layer in &prepared.scope.layers {
        for (name, value, secret) in &layer.vars {
            if *secret || names.contains(&name.as_str()) {
                values.push(value.clone());
            }
        }
    }
    for (name, value) in input.globals.iter().chain(&input.runtime) {
        if names.contains(&name.as_str()) {
            values.push(value.clone());
        }
    }
    values.retain(|v| !v.is_empty());
    values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    values.dedup();
    values
}

/// Variables declared in the files that apply to this request (collection, the selected
/// environment, folders, request), lowest precedence first.
fn declared_vars<'a>(
    context: &'a RequestContext<'_>,
    input: &PrepareInput<'_>,
) -> impl Iterator<Item = &'a Variable> {
    let environment = input
        .environment
        .and_then(|name| context.collection.environment(name));
    context
        .collection
        .defaults
        .vars
        .iter()
        .chain(environment.into_iter().flat_map(|e| e.variables.iter()))
        .chain(context.folders.iter().flat_map(|f| f.defaults.vars.iter()))
        .chain(context.request.spec.vars.iter())
}

/// Replaces every secret, raw or percent-encoded, with [`MASK`].
fn mask(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        for form in [
            secret.clone(),
            percent_encode(secret, false),
            percent_encode(secret, true),
        ] {
            if !form.is_empty() && out.contains(&form) {
                out = out.replace(&form, MASK);
            }
        }
    }
    out
}

/// Like [`mask`]; additionally hides HTTP Basic credentials that contain a secret, since
/// base64 hides the secret from plain substring matching.
fn mask_header(value: &str, secrets: &[String]) -> String {
    if let Some((scheme, credentials)) = value.split_once(' ')
        && scheme.eq_ignore_ascii_case("basic")
        && let Some(decoded) = base64_decode(credentials.trim())
    {
        let decoded = String::from_utf8_lossy(&decoded);
        if secrets.iter().any(|s| decoded.contains(s.as_str())) {
            return format!("{scheme} {MASK}");
        }
    }
    mask(value, secrets)
}

/// RFC 3986 encoding of everything but unreserved characters; `plus` encodes space as `+`
/// (form encoding).
fn percent_encode(s: &str, plus: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' if plus => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Unresolved placeholders when preparation failed

/// `prepare` reports unresolved names only on success. When it fails (typically because an
/// unresolved placeholder left the URL invalid), the report still names them: this scans the
/// request's templates and the variables declared for it, following variable values
/// recursively. Dynamic or script-provided variables are not known here.
fn unresolved_fallback(context: &RequestContext<'_>, input: &PrepareInput<'_>) -> Vec<String> {
    let mut defined: Vec<(&str, &str)> = input
        .globals
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    defined.extend(
        declared_vars(context, input)
            .filter(|v| v.enabled)
            .filter_map(|v| Some((v.name.as_str(), v.value.as_deref()?))),
    );
    defined.extend(input.runtime.iter().map(|(n, v)| (n.as_str(), v.as_str())));
    // Highest precedence last: look up from the end.
    let lookup = |name: &str| {
        defined
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
    };

    let spec = &context.request.spec;
    let mut templates: Vec<&str> = vec![&spec.url];
    for p in spec.params.iter().filter(|p| p.enabled) {
        templates.extend([p.name.as_str(), p.value.as_str()]);
    }
    let header_sets = std::iter::once(&context.collection.defaults.headers)
        .chain(context.folders.iter().map(|f| &f.defaults.headers))
        .chain(std::iter::once(&spec.headers));
    for kv in header_sets.flatten().filter(|kv| kv.enabled) {
        templates.extend([kv.name.as_str(), kv.value.as_str()]);
    }
    let auth = std::iter::once(&spec.auth)
        .chain(context.folders.iter().rev().map(|f| &f.defaults.auth))
        .chain(std::iter::once(&context.collection.defaults.auth))
        .find(|a| !matches!(a, Auth::Inherit));
    match auth {
        Some(Auth::Basic { username, password }) => templates.extend([&**username, password]),
        Some(Auth::Bearer { token }) => templates.push(token),
        Some(Auth::ApiKey { key, value, .. }) => templates.extend([&**key, value]),
        _ => {}
    }
    match &spec.body {
        Body::Raw { data, .. } => templates.push(data),
        Body::FormUrlEncoded { fields } => {
            for kv in fields.iter().filter(|kv| kv.enabled) {
                templates.extend([kv.name.as_str(), kv.value.as_str()]);
            }
        }
        Body::Multipart { parts } => {
            for part in parts.iter().filter(|p| p.enabled) {
                templates.push(&part.name);
                templates.extend(part.values.iter().map(String::as_str));
            }
        }
        Body::File { path, .. } => templates.push(path),
        Body::None => {}
    }

    let mut unresolved = Vec::new();
    let mut visited = Vec::new();
    let mut stack: Vec<&str> = templates.into_iter().rev().collect();
    while let Some(template) = stack.pop() {
        let mut nested = Vec::new();
        for name in placeholders(template) {
            match lookup(name) {
                Some(value) => {
                    if !visited.contains(&name) {
                        visited.push(name);
                        nested.push(value);
                    }
                }
                None => {
                    if !unresolved.iter().any(|u: &String| u == name) {
                        unresolved.push(name.to_owned());
                    }
                }
            }
        }
        stack.extend(nested.into_iter().rev());
    }
    unresolved
}

/// Names inside `{{...}}`, trimmed, in order.
fn placeholders(template: &str) -> impl Iterator<Item = &str> {
    let mut rest = template;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find("{{")?;
            let after = &rest[start + 2..];
            let end = after.find("}}")?;
            let name = after[..end].trim();
            rest = &after[end + 2..];
            if !name.is_empty() && !name.contains('{') {
                return Some(name);
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Base64 (standard alphabet, padded). Small enough to not need a dependency.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = match chunk {
            [a, b, c] => u32::from_be_bytes([0, *a, *b, *c]),
            [a, b] => u32::from_be_bytes([0, *a, *b, 0]),
            [a] => u32::from_be_bytes([0, *a, 0, 0]),
            _ => unreachable!(),
        };
        let sextet = |i: u32| B64[((n >> (18 - 6 * i)) & 63) as usize] as char;
        out.push(sextet(0));
        out.push(sextet(1));
        out.push(if chunk.len() > 1 { sextet(2) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(3) } else { '=' });
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use bytes::Bytes;

    use super::*;
    use crate::model::{
        Environment, Folder, Item, KeyValue, RequestDefaults, RequestEntry, RequestSpec,
    };
    use crate::transport::{BodyStream, HttpBody, Response, ResponseHead};
    use crate::vars::{Scope, VarLayer, VarSource};

    /// The mock futures are always ready, so a busy poll with a no-op waker is enough.
    fn block_on<F: Future>(fut: F) -> F::Output {
        let mut fut = std::pin::pin!(fut);
        let mut cx = Context::from_waker(std::task::Waker::noop());
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    struct Chunks(VecDeque<Bytes>);

    impl futures_core::Stream for Chunks {
        type Item = Result<Bytes, TransportError>;
        fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.pop_front().map(Ok))
        }
    }

    /// Records the request it was given and answers with a fixed result.
    struct MockTransport {
        answer: Result<(u16, Vec<&'static [u8]>), TransportError>,
        seen: std::cell::RefCell<Option<HttpRequest>>,
    }

    impl MockTransport {
        fn new(answer: Result<(u16, Vec<&'static [u8]>), TransportError>) -> Self {
            Self {
                answer,
                seen: Default::default(),
            }
        }
    }

    impl Transport for MockTransport {
        async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
            let url = request.url.clone();
            *self.seen.borrow_mut() = Some(request);
            let (status, chunks) = self.answer.clone()?;
            let body: BodyStream =
                Box::pin(Chunks(chunks.into_iter().map(Bytes::from_static).collect()));
            Ok(Response {
                head: ResponseHead {
                    status,
                    version: HttpVersion::Http11,
                    headers: vec![("content-type".into(), "text/plain".into())],
                    url,
                    redirects: vec![],
                    timing: Timing::default(),
                },
                body,
            })
        }
    }

    struct NoFiles;

    impl FileSource for NoFiles {
        fn read(&self, path: &str) -> Result<Vec<u8>, String> {
            Err(format!("no file {path}"))
        }
    }

    fn secret(name: &str) -> Variable {
        Variable {
            name: name.into(),
            value: None,
            enabled: true,
            secret: true,
        }
    }

    fn collection() -> Collection {
        let request = RequestEntry {
            path: "api/get.yml".into(),
            spec: RequestSpec {
                name: "get".into(),
                method: "GET".into(),
                url: "{{baseUrl}}/items?key={{apiKey}}".into(),
                headers: vec![
                    KeyValue::new("X-Key", "{{apiKey}}"),
                    KeyValue::new("X-Missing", "{{nope}}"),
                ],
                ..Default::default()
            },
        };
        Collection {
            name: "test".into(),
            defaults: RequestDefaults {
                vars: vec![Variable::new("baseUrl", "{{scheme}}://{{host}}")],
                ..Default::default()
            },
            items: vec![Item::Folder(Folder {
                name: "api".into(),
                path: "api".into(),
                defaults: RequestDefaults {
                    auth: Auth::Bearer {
                        token: "{{token}}".into(),
                    },
                    ..Default::default()
                },
                items: vec![Item::Request(request)],
                ..Default::default()
            })],
            environments: vec![Environment {
                name: "Local".into(),
                variables: vec![Variable::new("scheme", "http"), secret("apiKey")],
            }],
        }
    }

    fn input() -> PrepareInput<'static> {
        PrepareInput {
            environment: Some("Local"),
            globals: vec![],
            runtime: vec![("apiKey".into(), "s3cr3t value".into())],
        }
    }

    /// What strand C's `prepare` would produce for `collection()` with `input()`.
    fn prepared() -> Prepared {
        Prepared {
            request: HttpRequest {
                method: "POST".into(),
                url: "http://localhost/items?key=s3cr3t%20value".into(),
                headers: vec![
                    ("X-Key".into(), "s3cr3t value".into()),
                    ("X-Missing".into(), "{{nope}}".into()),
                    ("Authorization".into(), "Bearer tok".into()),
                ],
                body: Some(HttpBody {
                    content_type: Some("application/json".into()),
                    bytes: Bytes::from_static(b"{}"),
                }),
                options: Default::default(),
            },
            auth: Auth::Bearer {
                token: "{{token}}".into(),
            },
            scope: Scope {
                layers: vec![VarLayer {
                    source: VarSource::Runtime,
                    vars: vec![("apiKey".into(), "s3cr3t value".into(), false)],
                }],
            },
            used: vec![VarUse {
                name: "apiKey".into(),
                source: VarSource::Runtime,
                secret: false,
            }],
            unresolved: vec!["nope".into()],
        }
    }

    fn base_report() -> SendReport {
        SendReport {
            version: REPORT_VERSION,
            request_path: "api/get.yml".into(),
            environment: Some("Local".into()),
            request: None,
            response: None,
            variables: vec![],
            unresolved: vec![],
            error: None,
        }
    }

    #[test]
    fn unknown_request_is_not_found() {
        let c = collection();
        let t = MockTransport::new(Err(TransportError::Timeout));
        let r = block_on(send(
            &c,
            "api/missing.yml",
            &input(),
            &NoFiles,
            &t,
            &SendOptions::default(),
        ));
        assert_eq!(r.version, REPORT_VERSION);
        assert_eq!(r.environment.as_deref(), Some("Local"));
        assert_eq!(r.error, Some(SendError::NotFound("api/missing.yml".into())));
        assert!(r.request.is_none() && r.response.is_none());
        assert!(t.seen.borrow().is_none());
    }

    #[test]
    fn prepare_error_still_reports_unresolved() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let t = MockTransport::new(Err(TransportError::Timeout));
        let err = PrepareError::InvalidUrl("http://{{host}}/items".into());
        let r = block_on(finish(
            base_report(),
            &ctx,
            &input(),
            Err(err.clone()),
            &t,
            &SendOptions::default(),
        ));
        assert_eq!(r.error, Some(SendError::Prepare(err.to_string())));
        // baseUrl -> {{scheme}}://{{host}}: host is missing; header {{nope}}; auth {{token}}.
        assert_eq!(r.unresolved, ["host", "nope", "token"]);
        assert!(r.request.is_none());
        assert!(t.seen.borrow().is_none(), "nothing may be sent");
    }

    #[test]
    fn transport_error_keeps_the_sent_request_and_masks_secrets() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let t = MockTransport::new(Err(TransportError::Connect("refused".into())));
        let r = block_on(finish(
            base_report(),
            &ctx,
            &input(),
            Ok(prepared()),
            &t,
            &SendOptions::default(),
        ));
        assert_eq!(
            r.error,
            Some(SendError::Transport(TransportError::Connect(
                "refused".into()
            )))
        );
        assert!(r.response.is_none());
        assert_eq!(r.unresolved, ["nope"]);
        assert_eq!(r.variables.len(), 1);
        let sent = r.request.unwrap();
        assert_eq!(sent.method, "POST");
        // apiKey is declared secret in the environment, so its runtime value is masked.
        assert_eq!(sent.url, "http://localhost/items?key=***");
        assert_eq!(
            sent.headers,
            [
                ("X-Key".to_owned(), "***".to_owned()),
                ("X-Missing".to_owned(), "{{nope}}".to_owned()),
                ("Authorization".to_owned(), "Bearer tok".to_owned()),
                ("Content-Type".to_owned(), "application/json".to_owned()),
            ]
        );
        assert_eq!(sent.body_size, 2);
        // The transport gets the real values.
        let seen = t.seen.borrow().clone().unwrap();
        assert_eq!(seen.headers[0].1, "s3cr3t value");
    }

    #[test]
    fn secret_values_come_from_every_source() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let mut p = prepared();
        p.scope.layers.push(VarLayer {
            source: VarSource::Request,
            vars: vec![("pw".into(), "hunter2".into(), true)],
        });
        p.used.push(VarUse {
            name: "other".into(),
            source: VarSource::Global,
            secret: true,
        });
        p.scope.layers[0]
            .vars
            .push(("other".into(), "zzz".into(), false));
        let s = secret_values(&ctx, &input(), &p);
        assert_eq!(s, ["s3cr3t value", "hunter2", "zzz"]);
    }

    #[test]
    fn masking() {
        let secrets = vec!["p@ss word".to_owned(), "tok".to_owned()];
        assert_eq!(mask("a=p%40ss%20word&b=tok", &secrets), "a=***&b=***");
        assert_eq!(mask("a=p%40ss+word", &secrets), "a=***");
        assert_eq!(mask("nothing here", &secrets), "nothing here");
        let basic = format!("Basic {}", base64_encode(b"alice:p@ss word"));
        assert_eq!(mask_header(&basic, &secrets), "Basic ***");
        let plain = format!("Basic {}", base64_encode(b"alice:public"));
        assert_eq!(mask_header(&plain, &secrets), plain);
        assert_eq!(mask_header("Bearer tok", &secrets), "Bearer ***");
        assert_eq!(mask("x", &[]), "x");
    }

    #[test]
    fn body_encoding() {
        assert_eq!(report_body(b"hi", false), ReportBody::Utf8("hi".into()));
        assert_eq!(
            report_body(&[0xff, 0x00], false),
            ReportBody::Base64("/wA=".into())
        );
        // "é" is c3 a9; cut after c3.
        assert_eq!(
            report_body(&[b'a', 0xc3], true),
            ReportBody::Utf8("a".into())
        );
        assert_eq!(
            report_body(&[b'a', 0xc3], false),
            ReportBody::Base64("YcM=".into())
        );
    }

    #[test]
    fn report_json_shape() {
        let body = serde_json::to_value(ReportBody::Utf8("x".into())).unwrap();
        assert_eq!(body, serde_json::json!({"encoding": "utf8", "data": "x"}));
        let err = serde_json::to_value(SendError::NotFound("a.yml".into())).unwrap();
        assert_eq!(
            err,
            serde_json::json!({"kind": "not-found", "message": "a.yml"})
        );
        let err = serde_json::to_value(SendError::Transport(TransportError::Timeout)).unwrap();
        assert_eq!(
            err,
            serde_json::json!({"kind": "transport", "message": {"kind": "timeout"}})
        );
    }

    #[test]
    fn received_maps_every_field() {
        let c = CompletedResponse {
            head: ResponseHead {
                status: 404,
                version: HttpVersion::Http2,
                headers: vec![("a".into(), "b".into())],
                url: "http://x/final".into(),
                redirects: vec![Redirect {
                    status: 302,
                    location: "/final".into(),
                }],
                timing: Timing {
                    ttfb: Duration::from_millis(3),
                    ..Default::default()
                },
            },
            body: Bytes::from_static(b"abc"),
            truncated: true,
            size: 10,
            total: Duration::from_millis(7),
        };
        let r = received(c);
        assert_eq!(r.status, 404);
        assert_eq!(r.version, HttpVersion::Http2);
        assert_eq!(r.url, "http://x/final");
        assert_eq!(r.redirects.len(), 1);
        assert_eq!(r.body, ReportBody::Utf8("abc".into()));
        assert_eq!((r.size, r.truncated), (10, true));
        assert_eq!(r.timing.ttfb, Duration::from_millis(3));
        assert_eq!(r.total, Duration::from_millis(7));
    }

    #[test]
    fn base64_round_trip() {
        for (raw, enc) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(raw), enc);
            assert_eq!(base64_decode(enc).unwrap(), raw);
        }
        assert!(base64_decode("not base64!").is_none());
    }

    #[test]
    fn placeholder_scan() {
        let names: Vec<_> = placeholders("{{a}}/{{ b }}/{{}}{{c").collect();
        assert_eq!(names, ["a", "b"]);
    }

    /// The success path runs through `transport::collect` (strand A).
    #[test]
    fn response_is_collected_and_reported() {
        let c = collection();
        let ctx = c.request("api/get.yml").unwrap();
        let t = MockTransport::new(Ok((200, vec![b"hel", b"lo"])));
        let r = block_on(finish(
            base_report(),
            &ctx,
            &input(),
            Ok(prepared()),
            &t,
            &SendOptions { body_limit: 4 },
        ));
        assert_eq!(r.error, None);
        let resp = r.response.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, ReportBody::Utf8("hell".into()));
        assert_eq!((resp.size, resp.truncated), (5, true));
        assert!(resp.total >= resp.timing.ttfb);
    }

    /// End to end through `prepare` (strand C) and `collect` (strand A).
    #[test]
    fn send_prepares_sends_and_masks() {
        let c = collection();
        let t = MockTransport::new(Ok((201, vec![b"ok"])));
        let mut input = input();
        input.runtime.push(("host".into(), "localhost".into()));
        input.runtime.push(("token".into(), "tok".into()));
        let r = block_on(send(
            &c,
            "api/get.yml",
            &input,
            &NoFiles,
            &t,
            &SendOptions::default(),
        ));
        assert_eq!(r.error, None, "{r:?}");
        assert_eq!(r.unresolved, ["nope"]);
        let sent = r.request.unwrap();
        assert!(
            sent.url.starts_with("http://localhost/items?key="),
            "{}",
            sent.url
        );
        assert!(!sent.url.contains("s3cr3t"), "{}", sent.url);
        assert!(
            sent.headers
                .iter()
                .any(|(n, v)| n.eq_ignore_ascii_case("x-key") && v == MASK),
            "{:?}",
            sent.headers
        );
        let seen = t.seen.borrow().clone().unwrap();
        assert!(
            seen.headers
                .iter()
                .any(|(n, v)| n.eq_ignore_ascii_case("authorization") && v == "Bearer tok")
        );
        assert_eq!(r.response.unwrap().status, 201);
    }
}
