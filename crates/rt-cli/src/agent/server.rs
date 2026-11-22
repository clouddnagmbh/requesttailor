// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The companion's HTTP/1.1 server: checks, CORS, the endpoints (status, pair, send, and
//! secrets with `--allow-secrets`).
//!
//! Runs on a current-thread runtime inside a `LocalSet`: the native transport's body stream
//! is not `Send`, and one thread is plenty for a single user's requests.

use std::cell::RefCell;
use std::convert::Infallible;
use std::net::IpAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Limited};
use hyper::body::{Body, Frame, Incoming, SizeHint};
use hyper::header::{self, HeaderMap, HeaderName, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rt_core::transport::companion::{
    HEAD_HEADER, PROTOCOL_VERSION, PairRequest, PairResponse, SecretsError, SecretsRequest,
    SecretsResponse, Status, WireHead, WireRequest, encode_head,
};
use rt_core::transport::native::NativeTransport;
use rt_core::transport::{BodyStream, HttpRequest, Transport, TransportError};
use serde::Serialize;
use tokio::net::TcpListener;
use web_time::Instant;

use super::pairing::{CODE_TTL, Pairing};

/// Largest accepted `/v1/pair` request.
const MAX_PAIR_REQUEST: usize = 4 * 1024;
/// Largest accepted `/v1/secrets` request (references only).
const MAX_SECRETS_REQUEST: usize = 64 * 1024;
/// A client must send its request head within this time.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a browser may cache a preflight answer.
const PREFLIGHT_MAX_AGE: &str = "600";
/// A rejected request's unread body is read and dropped up to this size (and time), so the
/// connection closes cleanly and the client sees the answer instead of a reset.
const DRAIN_LIMIT: usize = 64 * 1024;
const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

const STATUS: &str = "/v1/status";
const PAIR: &str = "/v1/pair";
const SEND: &str = "/v1/send";
const SECRETS: &str = "/v1/secrets";

pub struct Agent {
    port: u16,
    origins: Vec<String>,
    /// Accepted `Host` headers (DNS rebinding defense).
    hosts: [String; 2],
    pairing: RefCell<Pairing>,
    transport: NativeTransport,
    /// `POST /v1/secrets` is answered (`--allow-secrets`).
    allow_secrets: bool,
}

impl Agent {
    pub fn new(port: u16, origins: Vec<String>, allow_secrets: bool) -> Result<Rc<Self>, String> {
        let transport = NativeTransport::new().map_err(|e| e.to_string())?;
        Ok(Rc::new(Self {
            port,
            origins,
            hosts: [format!("127.0.0.1:{port}"), format!("localhost:{port}")],
            pairing: RefCell::new(Pairing::new(Instant::now())),
            transport,
            allow_secrets,
        }))
    }

    pub fn print_banner(&self) {
        println!(
            "rt agent {}: companion for the RequestTailor web app",
            env!("CARGO_PKG_VERSION")
        );
        println!(
            "Listening on http://127.0.0.1:{} (this machine only)",
            self.port
        );
        println!("Allowed origins: {}", self.origins.join(", "));
        if self.allow_secrets {
            println!(
                "External secrets: on (paired origins can read secrets with your op, vault, aws, az and gcloud logins)"
            );
        }
        self.print_code();
        println!("Stop with Ctrl+C.");
    }

    /// Prints the current pairing code and one pairing link per allowed origin. Tests and
    /// scripts look for the line starting with `Pairing code: `.
    fn print_code(&self) {
        let pairing = self.pairing.borrow();
        println!(
            "Pairing code: {} (single use, valid for {} minutes)",
            pairing.code(),
            CODE_TTL.as_secs() / 60
        );
        println!("To pair, open a pairing link in the browser:");
        for origin in &self.origins {
            println!("  {origin}/#rt-pair={}", pairing.link_code());
        }
    }

    fn host_allowed(&self, req: &Request<()>) -> bool {
        // Only origin-form targets (`/v1/...`); an absolute URI would carry its own host.
        if req.uri().authority().is_some() || req.uri().scheme().is_some() {
            return false;
        }
        match single(req.headers(), header::HOST) {
            Ok(Some(host)) => self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)),
            _ => false,
        }
    }

    /// Whether `url` points at this companion itself (it never sends requests to itself).
    fn is_self(&self, url: &str) -> bool {
        let Ok(url) = url::Url::parse(url) else {
            return false;
        };
        let local = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified(),
            Some(url::Host::Ipv6(ip)) => {
                let ip = IpAddr::V6(ip);
                ip.is_loopback() || ip.is_unspecified()
            }
            Some(url::Host::Domain(d)) => {
                d.eq_ignore_ascii_case("localhost")
                    || d.to_ascii_lowercase().ends_with(".localhost")
            }
            None => false,
        };
        local && url.port_or_known_default() == Some(self.port)
    }
}

/// Accepts connections until the process ends.
pub async fn serve(listener: TcpListener, agent: Rc<Agent>) -> std::io::Result<()> {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                // E.g. too many open files: wait a moment instead of spinning.
                eprintln!("rt agent: accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let agent = agent.clone();
        tokio::task::spawn_local(async move {
            let service = hyper::service::service_fn(move |req| {
                let agent = agent.clone();
                async move { Ok::<_, Infallible>(handle(&agent, req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

async fn handle(agent: &Agent, req: Request<Incoming>) -> Response<AgentBody> {
    let (parts, body) = req.into_parts();
    let req = Request::from_parts(parts, ());
    let mut body = Some(body);
    let response = route(agent, &req, &mut body).await;
    if let Some(body) = body {
        let drain = Limited::new(body, DRAIN_LIMIT).collect();
        let _ = tokio::time::timeout(DRAIN_TIMEOUT, drain).await;
    }
    response
}

/// Checks and dispatch. Endpoints that read the body take it out of `body`.
async fn route(
    agent: &Agent,
    req: &Request<()>,
    body: &mut Option<Incoming>,
) -> Response<AgentBody> {
    let path = req.uri().path().to_owned();
    let method = req.method().clone();

    // 1. Host: a page on a rebound DNS name reaches us with its own name as Host.
    if !agent.host_allowed(req) {
        log(format_args!(
            "rejected {method} {path}: forbidden Host header"
        ));
        return text(StatusCode::FORBIDDEN, "forbidden host", None);
    }

    // 2. Origin: only allowlisted origins get an answer they can read (CORS).
    let origin = match single(req.headers(), header::ORIGIN) {
        Ok(origin) => origin.map(str::to_owned),
        Err(()) => return text(StatusCode::FORBIDDEN, "bad Origin header", None),
    };
    if let Some(o) = &origin
        && !agent.origins.contains(o)
    {
        log(format_args!(
            "rejected {method} {path} from origin {o}: not allowed (see --allow-origin)"
        ));
        return text(StatusCode::FORBIDDEN, "origin not allowed", None);
    }
    let Some(origin) = origin else {
        // No Origin: not a browser page. Only the status is public (for `curl` checks).
        if method == Method::GET && path == STATUS {
            return status(agent, None, req.headers());
        }
        log(format_args!("rejected {method} {path}: no Origin header"));
        return text(StatusCode::FORBIDDEN, "Origin header required", None);
    };
    let cors = Some(origin.as_str());

    if method == Method::OPTIONS {
        return preflight(req, &path, &origin);
    }
    match (method, path.as_str()) {
        (Method::GET, STATUS) => status(agent, Some(&origin), req.headers()),
        (Method::POST, PAIR) => pair(agent, body, &origin).await,
        (Method::POST, SEND) => send(agent, req, body, &origin).await,
        (Method::POST, SECRETS) => secrets(agent, req, body, &origin).await,
        (_, STATUS | PAIR | SEND | SECRETS) => {
            text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed", cors)
        }
        _ => text(StatusCode::NOT_FOUND, "not found", cors),
    }
}

/// CORS preflight, including Chrome's Private Network Access request
/// (`Access-Control-Request-Private-Network`). Only reached for allowed origins.
fn preflight(req: &Request<()>, path: &str, origin: &str) -> Response<AgentBody> {
    let expected = match path {
        STATUS => "GET",
        PAIR | SEND | SECRETS => "POST",
        _ => return text(StatusCode::NOT_FOUND, "not found", Some(origin)),
    };
    let headers = req.headers();
    let method_ok = header_str(headers, header::ACCESS_CONTROL_REQUEST_METHOD) == Some(expected);
    let headers_ok = header_str(headers, header::ACCESS_CONTROL_REQUEST_HEADERS)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .all(|h| h.eq_ignore_ascii_case("authorization") || h.eq_ignore_ascii_case("content-type"));
    if !method_ok || !headers_ok {
        return text(StatusCode::FORBIDDEN, "preflight not allowed", None);
    }
    let mut response = empty(StatusCode::NO_CONTENT, Some(origin));
    let h = response.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(expected),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    h.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static(PREFLIGHT_MAX_AGE),
    );
    if header_str(headers, "access-control-request-private-network") == Some("true") {
        h.insert(
            HeaderName::from_static("access-control-allow-private-network"),
            HeaderValue::from_static("true"),
        );
    }
    response
}

fn status(agent: &Agent, origin: Option<&str>, headers: &HeaderMap) -> Response<AgentBody> {
    let paired = match (origin, bearer(headers)) {
        (Some(origin), Some(token)) => agent.pairing.borrow().token_valid(origin, token),
        _ => false,
    };
    let status = Status {
        protocol: PROTOCOL_VERSION,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        paired,
    };
    json(StatusCode::OK, &status, origin)
}

async fn pair(agent: &Agent, body: &mut Option<Incoming>, origin: &str) -> Response<AgentBody> {
    let cors = Some(origin);
    let request: PairRequest = match read_json(body, MAX_PAIR_REQUEST).await {
        Ok(r) => r,
        Err(response) => return with_cors(response, origin),
    };
    let attempt = agent
        .pairing
        .borrow_mut()
        .attempt(origin, &request.code, Instant::now());
    match &attempt.token {
        Some(_) => log(format_args!("paired {origin}")),
        None => log(format_args!(
            "pairing from {origin} rejected: {}",
            attempt.reason.unwrap_or("wrong code")
        )),
    }
    if attempt.new_code.is_some() {
        agent.print_code();
    }
    match attempt.token {
        Some(token) => json(StatusCode::OK, &PairResponse { token }, cors),
        None => json(
            StatusCode::FORBIDDEN,
            &serde_json::json!({ "error": attempt.reason.unwrap_or("wrong code") }),
            cors,
        ),
    }
}

async fn send(
    agent: &Agent,
    req: &Request<()>,
    body: &mut Option<Incoming>,
    origin: &str,
) -> Response<AgentBody> {
    let cors = Some(origin);
    let authorized =
        bearer(req.headers()).is_some_and(|t| agent.pairing.borrow().token_valid(origin, t));
    if !authorized {
        log(format_args!(
            "rejected POST {SEND} from {origin}: missing or invalid token"
        ));
        return text(StatusCode::UNAUTHORIZED, "pairing required", cors);
    }
    let wire: WireRequest = match read_json(body, super::MAX_SEND_REQUEST).await {
        Ok(w) => w,
        Err(response) => return with_cors(response, origin),
    };
    let request = match HttpRequest::try_from(wire) {
        Ok(r) => r,
        Err(e) => return text(StatusCode::BAD_REQUEST, &e, cors),
    };
    let method = request.method.clone();
    let target = log_target(&request.url);
    if agent.is_self(&request.url) {
        log(format_args!(
            "rejected {method} {target}: the companion itself"
        ));
        return text(
            StatusCode::FORBIDDEN,
            "the companion does not send requests to itself",
            cors,
        );
    }
    let started = Instant::now();
    match agent.transport.send(request).await {
        Ok(response) => {
            log(format_args!(
                "{method} {target} -> {} ({} ms to first byte)",
                response.head.status,
                started.elapsed().as_millis()
            ));
            let Ok(head) = HeaderValue::from_str(&encode_head(&WireHead::from(&response.head)))
            else {
                let e = TransportError::Other("response head cannot be encoded".to_owned());
                return json(StatusCode::BAD_GATEWAY, &e, cors);
            };
            let mut reply = Response::new(AgentBody::Stream(response.body));
            let h = reply.headers_mut();
            h.insert(HEAD_HEADER, head);
            h.insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static(HEAD_HEADER),
            );
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            with_cors(reply, origin)
        }
        Err(e) => {
            log(format_args!("{method} {target} -> {e}"));
            json(StatusCode::BAD_GATEWAY, &e, cors)
        }
    }
}

/// Reads external secrets with the secret managers' CLIs (`rt_app::secret_sources`), on a
/// blocking thread (a CLI may take seconds, e.g. for a biometric prompt). The log shows the
/// provider and the number of variables, never values.
async fn secrets(
    agent: &Agent,
    req: &Request<()>,
    body: &mut Option<Incoming>,
    origin: &str,
) -> Response<AgentBody> {
    use rt_app::secret_sources::{
        DEFAULT_TIMEOUT, ProcessRunner, active, provider_label, resolve_one,
    };

    let cors = Some(origin);
    let authorized =
        bearer(req.headers()).is_some_and(|t| agent.pairing.borrow().token_valid(origin, t));
    if !authorized {
        log(format_args!(
            "rejected POST {SECRETS} from {origin}: missing or invalid token"
        ));
        return text(StatusCode::UNAUTHORIZED, "pairing required", cors);
    }
    if !agent.allow_secrets {
        log(format_args!(
            "rejected POST {SECRETS} from {origin}: external secrets are off (--allow-secrets)"
        ));
        let error = "The companion does not read external secrets. Restart it with \
                     `rt agent --allow-secrets` and pair again."
            .to_owned();
        return json(StatusCode::FORBIDDEN, &SecretsError { error }, cors);
    }
    let request: SecretsRequest = match read_json(body, MAX_SECRETS_REQUEST).await {
        Ok(r) => r,
        Err(response) => return with_cors(response, origin),
    };
    let label = provider_label(request.provider);
    let result = tokio::task::spawn_blocking(move || {
        let runner = ProcessRunner::default();
        let secrets = rt_core::model::ExternalSecrets {
            provider: request.provider,
            variables: request.variables,
        };
        active(&secrets)
            .map(|s| {
                resolve_one(&runner, secrets.provider, s, DEFAULT_TIMEOUT)
                    .map(|v| (s.name.clone(), v))
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .await
    .unwrap_or_else(|e| Err(format!("{label}: reading the secrets failed: {e}")));
    match result {
        Ok(values) => {
            log(format_args!(
                "read {} secret(s) from {label} for {origin}",
                values.len()
            ));
            json(StatusCode::OK, &SecretsResponse { values }, cors)
        }
        Err(error) => {
            log(format_args!(
                "reading secrets from {label} for {origin} failed"
            ));
            json(
                StatusCode::UNPROCESSABLE_ENTITY,
                &SecretsError { error },
                cors,
            )
        }
    }
}

/// `host[:port]` of a URL: what the log shows of a request (no path, no query).
fn log_target(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => match (u.host_str(), u.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            _ => "(no host)".to_owned(),
        },
        Err(_) => "(invalid URL)".to_owned(),
    }
}

fn log(message: std::fmt::Arguments<'_>) {
    eprintln!("rt agent: {message}");
}

/// The header's value if it occurs at most once; `Err` if repeated or not visible ASCII.
fn single(
    headers: &HeaderMap,
    name: impl header::AsHeaderName + Clone,
) -> Result<Option<&str>, ()> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(());
    }
    first.map(|v| v.to_str().map_err(|_| ())).transpose()
}

fn header_str(headers: &HeaderMap, name: impl header::AsHeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = single(headers, header::AUTHORIZATION).ok()??;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}

async fn read_json<T: serde::de::DeserializeOwned>(
    body: &mut Option<Incoming>,
    limit: usize,
) -> Result<T, Response<AgentBody>> {
    // Announced as too large (Content-Length): refuse unread, the caller drains a little.
    if body
        .as_ref()
        .is_some_and(|b| b.size_hint().lower() > limit as u64)
    {
        return Err(text(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request too large",
            None,
        ));
    }
    let Some(body) = body.take() else {
        return Err(text(StatusCode::BAD_REQUEST, "no request body", None));
    };
    let bytes = Limited::new(body, limit)
        .collect()
        .await
        .map_err(|e| {
            if e.downcast_ref::<http_body_util::LengthLimitError>()
                .is_some()
            {
                text(StatusCode::PAYLOAD_TOO_LARGE, "request too large", None)
            } else {
                text(StatusCode::BAD_REQUEST, "cannot read request body", None)
            }
        })?
        .to_bytes();
    serde_json::from_slice(&bytes)
        .map_err(|e| text(StatusCode::BAD_REQUEST, &format!("invalid JSON: {e}"), None))
}

fn with_cors(mut response: Response<AgentBody>, origin: &str) -> Response<AgentBody> {
    let h = response.headers_mut();
    // Origins were checked against the allowlist, so they are valid header values.
    if let Ok(value) = HeaderValue::from_str(origin) {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        h.append(header::VARY, HeaderValue::from_static("Origin"));
    }
    response
}

fn finish(
    mut response: Response<AgentBody>,
    status: StatusCode,
    cors: Option<&str>,
) -> Response<AgentBody> {
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    match cors {
        Some(origin) => with_cors(response, origin),
        None => response,
    }
}

fn empty(status: StatusCode, cors: Option<&str>) -> Response<AgentBody> {
    finish(Response::new(AgentBody::Full(None)), status, cors)
}

fn text(status: StatusCode, message: &str, cors: Option<&str>) -> Response<AgentBody> {
    let mut response = Response::new(AgentBody::Full(Some(Bytes::from(format!("{message}\n")))));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    finish(response, status, cors)
}

fn json(status: StatusCode, value: &impl Serialize, cors: Option<&str>) -> Response<AgentBody> {
    let body = serde_json::to_vec(value).expect("serializes");
    let mut response = Response::new(AgentBody::Full(Some(Bytes::from(body))));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    finish(response, status, cors)
}

/// Response body: a small fixed body, or the upstream body streamed through unchanged.
enum AgentBody {
    Full(Option<Bytes>),
    Stream(BodyStream),
}

impl Body for AgentBody {
    type Data = Bytes;
    type Error = TransportError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, TransportError>>> {
        match self.get_mut() {
            AgentBody::Full(bytes) => Poll::Ready(bytes.take().map(|b| Ok(Frame::data(b)))),
            AgentBody::Stream(stream) => stream
                .as_mut()
                .poll_next(cx)
                .map(|item| item.map(|chunk| chunk.map(Frame::data))),
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self, AgentBody::Full(None))
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            AgentBody::Full(bytes) => {
                SizeHint::with_exact(bytes.as_ref().map_or(0, |b| b.len() as u64))
            }
            AgentBody::Stream(_) => SizeHint::default(),
        }
    }
}
