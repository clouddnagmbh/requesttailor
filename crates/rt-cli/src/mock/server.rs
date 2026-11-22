// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The HTTP/1.1 server of `rt mock`: reads a request, asks the router, adds CORS headers.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;

use super::router::{Answer, Mock, MockRequest};

/// Largest request body the mock reads (bodies are only compared with saved examples).
const MAX_BODY: usize = 16 * 1024 * 1024;
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Accepts connections until the process ends.
pub async fn serve(listener: TcpListener, mock: Arc<Mock>) -> std::io::Result<()> {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                eprintln!("rt mock: accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let mock = mock.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let mock = mock.clone();
                async move { Ok::<_, Infallible>(handle(&mock, req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

async fn handle(mock: &Mock, req: Request<Incoming>) -> Response<Full<Bytes>> {
    let (parts, body) = req.into_parts();
    let preflight = parts.method == Method::OPTIONS
        && parts.headers.contains_key("access-control-request-method");
    let mut response = if preflight {
        let mut r = Response::new(Full::new(Bytes::new()));
        *r.status_mut() = StatusCode::NO_CONTENT;
        r
    } else {
        let body = match Limited::new(body, MAX_BODY).collect().await {
            Ok(b) => b.to_bytes(),
            Err(_) => {
                let mut r = Response::new(Full::new(Bytes::from_static(
                    b"{\"error\": \"request body too large or unreadable\"}",
                )));
                *r.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
                cors(&parts.headers, &mut r);
                return r;
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
        let answer = mock.answer(&request);
        log(&request, &answer);
        to_response(answer)
    };
    cors(&parts.headers, &mut response);
    response
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

fn to_response(answer: Answer) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::from(answer.body)));
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
fn cors(req: &hyper::HeaderMap, r: &mut Response<Full<Bytes>>) {
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
