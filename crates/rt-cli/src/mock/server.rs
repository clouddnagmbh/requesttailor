// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The HTTP/1.1 server of `rt mock`: reads a request, asks the router, adds CORS headers.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;

use super::hook::{AbortKind, HookAction, MockHook};
use super::router::{Answer, Mock, MockRequest};

/// The body of every answer: whole, or (slow body hook) in pieces.
type Body = BoxBody<Bytes, std::io::Error>;
/// A service error ends the connection without an answer (abort hooks).
#[derive(Debug)]
struct ServiceError;

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection dropped by a hook")
    }
}

impl std::error::Error for ServiceError {}

/// Largest request body the mock reads (bodies are only compared with saved examples).
const MAX_BODY: usize = 16 * 1024 * 1024;
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Accepts connections until the process ends. `hooks` (wave 18, [`MockHook`]) decide per
/// request before the examples answer; without hooks nothing changes.
pub async fn serve(
    listener: TcpListener,
    mock: Arc<Mock>,
    hooks: Arc<Vec<Arc<dyn MockHook>>>,
) -> std::io::Result<()> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                eprintln!("rt mock: accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let mock = mock.clone();
        let hooks = hooks.clone();
        let loopback = peer.ip().is_loopback();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let mock = mock.clone();
                let hooks = hooks.clone();
                async move { handle(&mock, &hooks, loopback, req).await }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

fn whole(bytes: Bytes) -> Body {
    Full::new(bytes).map_err(|never| match never {}).boxed()
}

async fn handle(
    mock: &Mock,
    hooks: &[Arc<dyn MockHook>],
    loopback: bool,
    req: Request<Incoming>,
) -> Result<Response<Body>, ServiceError> {
    let (parts, body) = req.into_parts();
    let preflight = parts.method == Method::OPTIONS
        && parts.headers.contains_key("access-control-request-method");
    let mut response = if preflight {
        let mut r = Response::new(whole(Bytes::new()));
        *r.status_mut() = StatusCode::NO_CONTENT;
        r
    } else {
        let body = match Limited::new(body, MAX_BODY).collect().await {
            Ok(b) => b.to_bytes(),
            Err(_) => {
                let mut r = Response::new(whole(Bytes::from_static(
                    b"{\"error\": \"request body too large or unreadable\"}",
                )));
                *r.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
                cors(&parts.headers, &mut r);
                return Ok(r);
            }
        };
        let headers: Vec<(String, String)> = parts
            .headers
            .iter()
            .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect();
        let request = MockRequest {
            method: parts.method.as_str(),
            path: parts.uri.path(),
            query: parts.uri.query().unwrap_or(""),
            headers,
            body: &body,
        };
        // Wave 18: the endpoints of the hooks (loopback clients only), then their decision.
        let endpoint = if loopback {
            hooks.iter().find_map(|h| h.endpoint(&request))
        } else {
            None
        };
        if let Some(answer) = endpoint {
            to_response(answer)
        } else {
            let action = hooks
                .iter()
                .map(|h| h.on_request(&request))
                .find(|a| *a != HookAction::Pass)
                .unwrap_or(HookAction::Pass);
            match act(action, mock, &request).await {
                Acted::Response(response) => response,
                Acted::Abort => {
                    eprintln!(
                        "{} {} -> connection dropped by a hook",
                        request.method, request.path
                    );
                    return Err(ServiceError);
                }
            }
        }
    };
    cors(&parts.headers, &mut response);
    Ok(response)
}

enum Acted {
    Response(Response<Body>),
    Abort,
}

/// Carries out what a hook decided.
async fn act(action: HookAction, mock: &Mock, request: &MockRequest<'_>) -> Acted {
    let mut action = action;
    loop {
        match action {
            HookAction::Pass => {
                let answer = mock.answer(request);
                log(request, &answer);
                return Acted::Response(to_response(answer));
            }
            HookAction::Delay(wait) => {
                tokio::time::sleep(wait).await;
                action = HookAction::Pass;
            }
            HookAction::After(wait, then) => {
                tokio::time::sleep(wait).await;
                action = *then;
            }
            HookAction::Respond(answer) => {
                log(request, &answer);
                return Acted::Response(to_response(answer));
            }
            HookAction::SlowBody(bytes_per_second) => {
                let answer = mock.answer(request);
                log(request, &answer);
                return Acted::Response(slow(answer, bytes_per_second));
            }
            HookAction::Abort(AbortKind::Stall(wait)) => {
                tokio::time::sleep(wait).await;
                return Acted::Abort;
            }
            // Both end the connection; a refinement (headers first, a real reset) is the
            // business of the hook's own crate.
            HookAction::Abort(AbortKind::Reset | AbortKind::CloseAfterHeaders) => {
                return Acted::Abort;
            }
        }
    }
}

/// The answer with its body in ten pieces a second at about `bytes_per_second`.
fn slow(answer: Answer, bytes_per_second: u64) -> Response<Body> {
    let mut template = to_response(Answer {
        body: Vec::new(),
        ..answer.clone()
    });
    let piece = usize::try_from((bytes_per_second / 10).max(1)).unwrap_or(usize::MAX);
    let body = Bytes::from(answer.body);
    let stream = futures_util::stream::unfold(0usize, move |at| {
        let body = body.clone();
        async move {
            if at >= body.len() {
                return None;
            }
            if at > 0 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let end = at.saturating_add(piece).min(body.len());
            Some((
                Ok::<_, std::io::Error>(hyper::body::Frame::data(body.slice(at..end))),
                end,
            ))
        }
    });
    *template.body_mut() = StreamBody::new(stream).boxed();
    template
}

fn log(req: &MockRequest<'_>, answer: &Answer) {
    let query = if req.query.is_empty() {
        String::new()
    } else {
        format!("?{}", req.query)
    };
    let what = match &answer.matched {
        Some((request, example)) => format!("{request}: {example}"),
        None => "no example".to_owned(),
    };
    eprintln!(
        "{} {}{query} -> {} ({what})",
        req.method, req.path, answer.status
    );
}

fn to_response(answer: Answer) -> Response<Body> {
    let mut r = Response::new(whole(Bytes::from(answer.body)));
    *r.status_mut() = StatusCode::from_u16(answer.status).unwrap_or(StatusCode::OK);
    if let Some(reason) = answer.reason
        && !reason.is_empty()
        && r.status().canonical_reason() != Some(reason.as_str())
        && let Ok(reason) = hyper::ext::ReasonPhrase::try_from(reason)
    {
        r.extensions_mut().insert(reason);
    }
    for (name, value) in answer.headers {
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            r.headers_mut().append(n, v);
        }
    }
    r
}

/// The CORS headers `rt-testserver` sends: any origin, any method, the requested headers.
fn cors(req: &hyper::HeaderMap, r: &mut Response<Body>) {
    let h = r.headers_mut();
    h.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    h.insert(
        "access-control-expose-headers",
        HeaderValue::from_static("*"),
    );
    h.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS"),
    );
    // "*" does not cover Authorization in the Fetch spec, so echo what the browser asks for.
    let allow = req
        .get("access-control-request-headers")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("*"));
    h.insert("access-control-allow-headers", allow);
    h.insert("access-control-max-age", HeaderValue::from_static("600"));
}
