// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wire conversions and the browser-side unwrapping of companion answers. Runs natively; the
//! `fetch` call itself is covered by the web app's end-to-end test against a real `rt agent`.

use bytes::Bytes;

use super::super::{Response, TransportError, collect};
use super::*;

fn request(body: Option<HttpBody>) -> HttpRequest {
    HttpRequest {
        method: "PATCH".into(),
        url: "https://api.example/items/1?x=%20".into(),
        headers: vec![
            ("X-Case".into(), "Kept".into()),
            ("x-dup".into(), "1".into()),
            ("x-dup".into(), "2".into()),
        ],
        body,
        options: SendOptions {
            timeout: Some(Duration::from_millis(2500)),
            follow_redirects: false,
            max_redirects: 3,
        },
    }
}

#[test]
fn request_round_trip_with_binary_body() {
    let bytes: Vec<u8> = (0..=255u8).chain([0, 0xff, 0x80]).collect();
    let original = request(Some(HttpBody {
        content_type: Some("application/octet-stream".into()),
        bytes: Bytes::from(bytes.clone()),
    }));
    let wire = WireRequest::from(&original);
    assert_eq!(wire.body.as_deref(), Some(B64.encode(&bytes).as_str()));
    assert_eq!(wire.timeout_ms, Some(2500));
    // Through JSON, as on the wire.
    let json = serde_json::to_string(&wire).unwrap();
    let back: WireRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back, wire);
    assert_eq!(HttpRequest::try_from(back).unwrap(), original);
}

#[test]
fn request_round_trip_without_body_and_with_empty_body() {
    let none = request(None);
    let wire = WireRequest::from(&none);
    assert_eq!(
        (wire.body.as_deref(), wire.content_type.as_deref()),
        (None, None)
    );
    assert_eq!(HttpRequest::try_from(wire).unwrap(), none);

    let mut empty = request(Some(HttpBody {
        content_type: None,
        bytes: Bytes::new(),
    }));
    empty.options = SendOptions::default();
    let wire = WireRequest::from(&empty);
    assert_eq!(wire.body.as_deref(), Some(""));
    assert_eq!(wire.timeout_ms, None);
    assert_eq!(HttpRequest::try_from(wire).unwrap(), empty);
}

#[test]
fn invalid_wire_requests_are_rejected() {
    let mut wire = WireRequest::from(&request(None));
    wire.content_type = Some("text/plain".into());
    assert!(HttpRequest::try_from(wire.clone()).is_err());
    wire.body = Some("not base64!".into());
    assert!(HttpRequest::try_from(wire).unwrap_err().contains("base64"));
}

fn head() -> ResponseHead {
    ResponseHead {
        status: 201,
        version: HttpVersion::Http2,
        headers: vec![
            ("content-type".into(), "text/plain".into()),
            ("x-name".into(), "Grüße 🎉".into()),
        ],
        url: "https://api.example/ö".into(),
        redirects: vec![Redirect {
            status: 302,
            location: "/next".into(),
        }],
        timing: Timing {
            dns: Some(Duration::from_millis(1)),
            connect: None,
            tls: Some(Duration::from_millis(3)),
            ttfb: Duration::from_millis(12),
        },
    }
}

#[test]
fn head_round_trip_through_an_ascii_header() {
    let mut original = head();
    // DEL is ASCII but not allowed in a header value.
    let del = format!("a{}b", char::from(0x7f));
    original.headers.push(("x-del".into(), del));
    let value = encode_head(&WireHead::from(&original));
    assert!(value.bytes().all(|b| (b' '..=b'~').contains(&b)), "{value}");
    let escaped = |hex: &str| format!("{}u{hex}", '\\');
    assert!(value.contains(&format!("Gr{}", escaped("00fc"))), "{value}");
    // The emoji is outside the BMP: a surrogate pair.
    let pair = format!("{}{}", escaped("d83c"), escaped("df89"));
    assert!(value.contains(&pair), "{value}");
    let back: ResponseHead = decode_head(&value).unwrap().into();
    assert_eq!(back, original);
    assert!(decode_head("{").is_err());
}

#[test]
fn envelope_carries_token_and_json() {
    let t = CompanionTransport::new("http://127.0.0.1:47812/", "tok");
    let original = request(None);
    let e = t.envelope(&original).unwrap();
    assert_eq!(e.method, "POST");
    assert_eq!(e.url, "http://127.0.0.1:47812/v1/send");
    assert_eq!(
        e.headers,
        [("Authorization".to_owned(), "Bearer tok".to_owned())]
    );
    assert_eq!(
        e.options.timeout,
        Some(Duration::from_millis(2500) + FETCH_GRACE)
    );
    assert!(!e.options.follow_redirects);
    let body = e.body.unwrap();
    assert_eq!(body.content_type.as_deref(), Some("application/json"));
    let wire: WireRequest = serde_json::from_slice(&body.bytes).unwrap();
    assert_eq!(HttpRequest::try_from(wire).unwrap(), original);
}

struct Chunks(Vec<Result<Bytes, TransportError>>);

impl futures_core::Stream for Chunks {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();
        std::task::Poll::Ready(if this.0.is_empty() {
            None
        } else {
            Some(this.0.remove(0))
        })
    }
}

fn reply(
    status: u16,
    headers: Vec<(String, String)>,
    chunks: Vec<Result<Bytes, TransportError>>,
) -> Response {
    Response {
        head: ResponseHead {
            status,
            version: HttpVersion::Unknown,
            headers,
            url: "http://127.0.0.1:47812/v1/send".into(),
            redirects: vec![],
            timing: Timing::default(),
        },
        body: Box::pin(Chunks(chunks)),
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

#[test]
fn unwraps_a_successful_answer() {
    let t = CompanionTransport::new("http://127.0.0.1:47812", "tok");
    let started = web_time::Instant::now();
    let ok = reply(
        200,
        // Header names are case-insensitive (fetch lowercases them).
        vec![("X-RT-Head".into(), encode_head(&WireHead::from(&head())))],
        vec![
            Ok(Bytes::from_static(b"he")),
            Ok(Bytes::from_static(b"llo")),
        ],
    );
    let done = block_on(async {
        let r = t.unwrap_response(ok, None, started).await.unwrap();
        collect(r, 100, started).await.unwrap()
    });
    assert_eq!(done.head, head());
    assert_eq!(&done.body[..], b"hello");
}

#[test]
fn unwraps_errors() {
    let t = CompanionTransport::new("http://127.0.0.1:47812", "tok");
    let started = web_time::Instant::now();
    let answer = |status, body: &'static [u8]| {
        let r = reply(status, vec![], vec![Ok(Bytes::from_static(body))]);
        block_on(t.unwrap_response(r, None, started)).unwrap_err()
    };
    assert_eq!(
        answer(502, br#"{"kind":"dns","message":"no such host"}"#),
        TransportError::Dns("no such host".into())
    );
    assert_eq!(
        answer(502, br#"{"kind":"timeout"}"#),
        TransportError::Timeout
    );
    assert!(matches!(answer(502, b"oops"), TransportError::Other(m) if m.contains("oops")));
    assert!(matches!(answer(401, b""), TransportError::Blocked(_)));
    // 200 without the head header.
    assert!(matches!(answer(200, b""), TransportError::Other(_)));
    assert_eq!(
        answer(403, b""),
        TransportError::Other("the companion answered HTTP 403".into())
    );
}

#[test]
fn broken_body_becomes_timeout_only_after_the_timeout() {
    let t = CompanionTransport::new("http://127.0.0.1:47812", "tok");
    let value = encode_head(&WireHead::from(&head()));
    let broken = |timeout| {
        let r = reply(
            200,
            vec![(HEAD_HEADER.into(), value.clone())],
            vec![
                Ok(Bytes::from_static(b"part")),
                Err(TransportError::Blocked("network error".into())),
            ],
        );
        let started = web_time::Instant::now();
        block_on(async {
            let r = t.unwrap_response(r, timeout, started).await?;
            collect(r, 100, started).await
        })
        .unwrap_err()
    };
    assert!(matches!(
        broken(Some(Duration::from_secs(60))),
        TransportError::Other(m) if m.contains("broke off")
    ));
    assert!(matches!(broken(None), TransportError::Other(_)));
    assert_eq!(broken(Some(Duration::ZERO)), TransportError::Timeout);
}
