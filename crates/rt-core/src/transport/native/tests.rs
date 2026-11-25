// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests for the native transport against the in-process test server
//! (`rt-testserver`) and a local TLS server (HTTP/2 via ALPN).

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures_core::Stream;

use super::NativeTransport;
use crate::transport::{
    CompletedResponse, HttpBody, HttpRequest, HttpVersion, Response, SendOptions, Transport,
    TransportError, collect,
};

fn request(method: &str, url: &str) -> HttpRequest {
    HttpRequest {
        method: method.into(),
        url: url.into(),
        headers: vec![],
        body: None,
        options: SendOptions::default(),
    }
}

async fn send(t: &NativeTransport, req: HttpRequest) -> Result<CompletedResponse, TransportError> {
    let started = web_time::Instant::now();
    let response = t.send(req).await?;
    collect(response, 16 * 1024 * 1024, started).await
}

fn json(c: &CompletedResponse) -> serde_json::Value {
    serde_json::from_slice(&c.body).expect("JSON body")
}

/// Values of a header in the test server's echo (`headers` is a list of `[name, value]`).
fn echoed(v: &serde_json::Value, name: &str) -> Vec<String> {
    v["headers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h[0] == name)
        .map(|h| h[1].as_str().unwrap().to_owned())
        .collect()
}

fn header<'a>(c: &'a CompletedResponse, name: &str) -> Option<&'a str> {
    c.head
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

async fn server() -> (NativeTransport, String) {
    let addr = rt_testserver::spawn_local().await;
    (NativeTransport::new().unwrap(), format!("http://{addr}"))
}

#[test]
fn send_future_is_send() {
    fn assert_send<T: Send>(_: &T) {}
    let t = NativeTransport::new().unwrap();
    let fut = t.send(request("GET", "http://127.0.0.1:1/"));
    assert_send(&fut);
}

#[tokio::test]
async fn echoes_method_headers_and_body() {
    let (t, base) = server().await;
    let mut req = request("POST", &format!("{base}/echo/a?b=1"));
    req.headers = vec![
        ("X-Custom".into(), "one".into()),
        ("X-Multi".into(), "a".into()),
        ("X-Multi".into(), "b".into()),
    ];
    req.body = Some(HttpBody {
        content_type: Some("text/plain".into()),
        bytes: Bytes::from_static(b"hello"),
    });
    let c = send(&t, req).await.unwrap();
    assert_eq!(c.head.status, 200);
    assert_eq!(c.head.version, HttpVersion::Http11);
    assert_eq!(c.head.url, format!("{base}/echo/a?b=1"));
    assert!(c.head.redirects.is_empty());
    assert_eq!(header(&c, "content-type"), Some("application/json"));
    let v = json(&c);
    assert_eq!(v["method"], "POST");
    assert_eq!(v["path"], "/echo/a");
    assert_eq!(v["query"], "b=1");
    assert_eq!(v["body"], "hello");
    assert_eq!(v["body_len"], 5);
    assert_eq!(echoed(&v, "x-custom"), ["one"]);
    assert_eq!(echoed(&v, "x-multi"), ["a", "b"]);
    assert_eq!(echoed(&v, "content-type"), ["text/plain"]);
    assert_eq!(echoed(&v, "content-length"), ["5"]);
    assert_eq!(echoed(&v, "host").len(), 1);
}

#[tokio::test]
async fn explicit_content_type_header_wins() {
    let (t, base) = server().await;
    let mut req = request("PUT", &format!("{base}/echo"));
    req.headers = vec![("content-TYPE".into(), "application/json".into())];
    req.body = Some(HttpBody {
        content_type: Some("text/plain".into()),
        bytes: Bytes::from_static(b"{}"),
    });
    let v = json(&send(&t, req).await.unwrap());
    assert_eq!(echoed(&v, "content-type"), ["application/json"]);
}

#[tokio::test]
async fn binary_body_round_trips() {
    let (t, base) = server().await;
    let bytes: Vec<u8> = (0..=255u8).collect();
    let mut req = request("POST", &format!("{base}/echo"));
    req.body = Some(HttpBody {
        content_type: None,
        bytes: Bytes::from(bytes.clone()),
    });
    let v = json(&send(&t, req).await.unwrap());
    assert_eq!(v["body_len"], 256);
    assert!(echoed(&v, "content-type").is_empty());
}

#[tokio::test]
async fn status_codes_pass_through() {
    let (t, base) = server().await;
    for code in [201u16, 204, 400, 401, 404, 418, 500, 503] {
        let c = send(&t, request("GET", &format!("{base}/status/{code}")))
            .await
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert_eq!(c.head.status, code);
    }
}

#[tokio::test]
async fn redirects_are_followed_and_recorded() {
    let (t, base) = server().await;
    let c = send(&t, request("GET", &format!("{base}/redirect/3")))
        .await
        .unwrap();
    assert_eq!(c.head.status, 200);
    assert_eq!(json(&c)["redirected"], true);
    assert_eq!(c.head.url, format!("{base}/redirect/0"));
    let hops: Vec<(u16, String)> = c
        .head
        .redirects
        .iter()
        .map(|r| (r.status, r.location.clone()))
        .collect();
    assert_eq!(
        hops,
        [
            (302, format!("{base}/redirect/2")),
            (302, format!("{base}/redirect/1")),
            (302, format!("{base}/redirect/0")),
        ]
    );
}

#[tokio::test]
async fn redirects_up_to_the_limit_then_error() {
    let (t, base) = server().await;
    let mut req = request("GET", &format!("{base}/redirect/2"));
    req.options.max_redirects = 2;
    let c = send(&t, req).await.unwrap();
    assert_eq!(c.head.redirects.len(), 2);

    let mut req = request("GET", &format!("{base}/redirect/3"));
    req.options.max_redirects = 2;
    assert_eq!(
        send(&t, req).await.unwrap_err(),
        TransportError::TooManyRedirects(2)
    );

    let mut req = request("GET", &format!("{base}/redirect/1"));
    req.options.max_redirects = 0;
    assert_eq!(
        send(&t, req).await.unwrap_err(),
        TransportError::TooManyRedirects(0)
    );
}

#[tokio::test]
async fn redirects_not_followed_when_disabled() {
    let (t, base) = server().await;
    let mut req = request("GET", &format!("{base}/redirect/2"));
    req.options.follow_redirects = false;
    let c = send(&t, req).await.unwrap();
    assert_eq!(c.head.status, 302);
    assert_eq!(header(&c, "location"), Some("/redirect/1"));
    assert!(c.head.redirects.is_empty());
    assert_eq!(c.head.url, format!("{base}/redirect/2"));
}

#[tokio::test]
async fn post_302_becomes_get_without_body() {
    let (t, base) = server().await;
    let mut req = request("POST", &format!("{base}/redirect-to?url=%2Fecho%2Fnext"));
    req.body = Some(HttpBody {
        content_type: Some("text/plain".into()),
        bytes: Bytes::from_static(b"payload"),
    });
    let c = send(&t, req).await.unwrap();
    let v = json(&c);
    assert_eq!(v["method"], "GET");
    assert_eq!(v["path"], "/echo/next");
    assert_eq!(v["body_len"], 0);
    assert!(echoed(&v, "content-type").is_empty());
    assert_eq!(c.head.redirects.len(), 1);
    assert_eq!(c.head.redirects[0].location, format!("{base}/echo/next"));
}

/// A server that answers every request with `status` and `Location: location`.
async fn redirect_server(status: u16, location: String) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                continue;
            };
            let location = location.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |_req: hyper::Request<_>| {
                    let location = location.clone();
                    async move {
                        let mut res = hyper::Response::new(http_body_util::Full::new(Bytes::new()));
                        *res.status_mut() = hyper::StatusCode::from_u16(status).unwrap();
                        res.headers_mut()
                            .insert("location", location.parse().unwrap());
                        Ok::<_, std::convert::Infallible>(res)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(tcp), service)
                    .await;
            });
        }
    });
    addr
}

#[tokio::test]
async fn redirect_method_and_body_rules() {
    let echo = rt_testserver::spawn_local().await;
    let t = NativeTransport::new().unwrap();
    for (status, method, expected_method, keeps_body) in [
        (307, "POST", "POST", true),
        (308, "PUT", "PUT", true),
        (303, "POST", "GET", false),
        (303, "PUT", "GET", false),
        (301, "POST", "GET", false),
        (301, "DELETE", "DELETE", true),
    ] {
        let target = format!("http://{echo}/echo/target");
        let addr = redirect_server(status, target.clone()).await;
        let mut req = request(method, &format!("http://{addr}/start"));
        req.body = Some(HttpBody {
            content_type: Some("text/plain".into()),
            bytes: Bytes::from_static(b"payload"),
        });
        let c = send(&t, req).await.unwrap();
        let v = json(&c);
        let case = format!("{status} {method}");
        assert_eq!(v["method"], expected_method, "{case}");
        assert_eq!(c.head.url, target, "{case}");
        assert_eq!(c.head.redirects.len(), 1, "{case}");
        assert_eq!(c.head.redirects[0].status, status, "{case}");
        if keeps_body {
            assert_eq!(v["body"], "payload", "{case}");
            assert_eq!(echoed(&v, "content-type"), ["text/plain"], "{case}");
        } else {
            assert_eq!(v["body_len"], 0, "{case}");
            assert!(echoed(&v, "content-type").is_empty(), "{case}");
        }
    }
}

#[tokio::test]
async fn redirect_without_location_is_returned_as_is() {
    let (t, base) = server().await;
    // 304 and 300 are not followed.
    let c = send(&t, request("GET", &format!("{base}/status/300")))
        .await
        .unwrap();
    assert_eq!(c.head.status, 300);
    assert!(c.head.redirects.is_empty());
}

#[tokio::test]
async fn cross_origin_redirect_drops_credentials() {
    let (t, base_a) = server().await;
    let b = rt_testserver::spawn_local().await;
    let target = format!("http://{b}/echo");
    let mut req = request("GET", &format!("{base_a}/redirect-to?url={target}"));
    req.headers = vec![
        ("Authorization".into(), "Bearer secret".into()),
        ("Cookie".into(), "session=1".into()),
        ("X-Keep".into(), "yes".into()),
    ];
    let c = send(&t, req).await.unwrap();
    assert_eq!(c.head.url, target);
    let v = json(&c);
    assert!(echoed(&v, "authorization").is_empty(), "{v}");
    assert!(echoed(&v, "cookie").is_empty(), "{v}");
    assert_eq!(echoed(&v, "x-keep"), ["yes"]);
}

#[tokio::test]
async fn same_origin_redirect_keeps_credentials() {
    let (t, base) = server().await;
    let mut req = request("GET", &format!("{base}/redirect-to?url=%2Fecho"));
    req.headers = vec![
        ("Authorization".into(), "Bearer secret".into()),
        ("Cookie".into(), "session=1".into()),
    ];
    let v = json(&send(&t, req).await.unwrap());
    assert_eq!(echoed(&v, "authorization"), ["Bearer secret"]);
    assert_eq!(echoed(&v, "cookie"), ["session=1"]);
}

#[tokio::test]
async fn timeout_is_reported() {
    let (t, base) = server().await;
    let mut req = request("GET", &format!("{base}/delay/3000"));
    req.options.timeout = Some(Duration::from_millis(200));
    let started = std::time::Instant::now();
    assert_eq!(t.send(req).await.unwrap_err(), TransportError::Timeout);
    assert!(started.elapsed() < Duration::from_millis(2000));
}

#[tokio::test]
async fn timeout_also_covers_reading_the_body() {
    let (t, base) = server().await;
    // 100 GB: the headers arrive at once, the body cannot finish within the timeout.
    let mut req = request("GET", &format!("{base}/bytes/107374182400"));
    req.options.timeout = Some(Duration::from_millis(300));
    let started = web_time::Instant::now();
    let response = t.send(req).await.unwrap();
    assert_eq!(response.head.status, 200);
    assert_eq!(
        collect(response, 1024, started).await.unwrap_err(),
        TransportError::Timeout
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn request_within_timeout_succeeds() {
    let (t, base) = server().await;
    let mut req = request("GET", &format!("{base}/delay/10"));
    req.options.timeout = Some(Duration::from_secs(10));
    assert_eq!(send(&t, req).await.unwrap().head.status, 200);
}

#[tokio::test]
async fn streams_a_1_mb_body_correctly() {
    let (t, base) = server().await;
    let n = 1024 * 1024;
    let c = send(&t, request("GET", &format!("{base}/bytes/{n}")))
        .await
        .unwrap();
    assert_eq!(c.size, n as u64);
    assert!(!c.truncated);
    assert_eq!(c.body.len(), n);
    assert!(
        c.body
            .iter()
            .enumerate()
            .all(|(i, b)| *b == (i % 251) as u8)
    );
    assert_eq!(header(&c, "content-type"), Some("application/octet-stream"));
}

/// Wraps a body and records how it is consumed, without keeping the data.
struct Probe {
    inner: crate::transport::BodyStream,
    chunks: Arc<AtomicUsize>,
    largest: Arc<AtomicU64>,
}

impl Stream for Probe {
    type Item = Result<Bytes, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let polled = self.inner.as_mut().poll_next(cx);
        if let Poll::Ready(Some(Ok(chunk))) = &polled {
            self.chunks.fetch_add(1, Ordering::Relaxed);
            self.largest
                .fetch_max(chunk.len() as u64, Ordering::Relaxed);
        }
        polled
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_100_mb_body_is_consumed_chunk_wise_and_stays_bounded() {
    let (t, base) = server().await;
    let n: u64 = 100 * 1024 * 1024;
    let limit = 64 * 1024;
    let started = web_time::Instant::now();
    let Response { head, body } = t
        .send(request("GET", &format!("{base}/bytes/{n}")))
        .await
        .unwrap();
    let chunks = Arc::new(AtomicUsize::new(0));
    let largest = Arc::new(AtomicU64::new(0));
    let probed = Response {
        head,
        body: Box::pin(Probe {
            inner: body,
            chunks: chunks.clone(),
            largest: largest.clone(),
        }),
    };
    let c = collect(probed, limit, started).await.unwrap();
    assert_eq!(c.size, n);
    assert!(c.truncated);
    assert_eq!(c.body.len(), limit);
    assert!(
        c.body
            .iter()
            .enumerate()
            .all(|(i, b)| *b == (i % 251) as u8)
    );
    // The body arrives as many small chunks; no single chunk comes close to the whole body,
    // so neither the transport nor `collect` buffers it.
    let largest = largest.load(Ordering::Relaxed);
    assert!(largest <= 1024 * 1024, "largest chunk {largest} bytes");
    assert!(chunks.load(Ordering::Relaxed) >= 100);
}

#[tokio::test]
async fn timing_for_a_fresh_connection_then_reuse() {
    let addr = rt_testserver::spawn_local().await;
    let t = NativeTransport::new().unwrap();
    // A host name, so a DNS lookup happens.
    let url = format!("http://localhost:{}/delay/20", addr.port());
    let first = send(&t, request("GET", &url)).await.unwrap();
    let timing = &first.head.timing;
    assert!(timing.dns.is_some(), "{timing:?}");
    assert!(timing.connect.is_some(), "{timing:?}");
    assert!(timing.tls.is_none(), "{timing:?}");
    assert!(timing.ttfb >= Duration::from_millis(20), "{timing:?}");
    assert!(first.total >= timing.ttfb);

    let second = send(&t, request("GET", &url)).await.unwrap();
    let timing = &second.head.timing;
    assert_eq!(timing.dns, None, "reused connection: {timing:?}");
    assert_eq!(timing.connect, None, "reused connection: {timing:?}");
    assert!(timing.ttfb >= Duration::from_millis(20), "{timing:?}");
}

#[tokio::test]
async fn ip_literal_has_no_dns_phase() {
    let (t, base) = server().await;
    let c = send(&t, request("GET", &format!("{base}/status/200")))
        .await
        .unwrap();
    assert_eq!(c.head.timing.dns, None);
    assert!(c.head.timing.connect.is_some());
}

#[tokio::test]
async fn clones_share_the_connection_pool() {
    let (t, base) = server().await;
    send(&t, request("GET", &format!("{base}/status/200")))
        .await
        .unwrap();
    let clone = t.clone();
    let c = send(&clone, request("GET", &format!("{base}/status/200")))
        .await
        .unwrap();
    assert_eq!(c.head.timing.connect, None);
}

#[tokio::test]
async fn invalid_urls_are_rejected() {
    let t = NativeTransport::new().unwrap();
    for url in ["not a url", "/relative", "ftp://example.test/", "http://"] {
        match t.send(request("GET", url)).await {
            Err(TransportError::InvalidUrl(_)) => {}
            other => panic!("{url}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn refused_connection_is_a_connect_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let t = NativeTransport::new().unwrap();
    match t
        .send(request("GET", &format!("http://127.0.0.1:{port}/")))
        .await
    {
        Err(TransportError::Connect(_)) => {}
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn unknown_host_is_a_dns_error() {
    let t = NativeTransport::new().unwrap();
    match t
        .send(request("GET", "http://does-not-exist.invalid/"))
        .await
    {
        Err(TransportError::Dns(_)) => {}
        other => panic!("{other:?}"),
    }
}

// --- TLS and HTTP/2 -------------------------------------------------------------------------

struct TlsServer {
    addr: SocketAddr,
    ca_der: Vec<u8>,
}

/// A local HTTPS server with a throwaway CA. It offers `h2` and `http/1.1` via ALPN (or only
/// `alpn`) and answers every request with the HTTP version it saw.
async fn tls_server(alpn: &[&[u8]]) -> TlsServer {
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer,
        KeyPair, KeyUsagePurpose,
    };

    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "RequestTailor test CA");
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf_key = KeyPair::generate().unwrap();
    let mut leaf_params =
        CertificateParams::new(vec!["localhost".to_owned(), "127.0.0.1".to_owned()]).unwrap();
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf_params.use_authority_key_identifier_extension = true;
    let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf_cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into()),
        )
        .unwrap();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                continue;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let service = hyper::service::service_fn(|req: hyper::Request<_>| async move {
                    let body = format!("{:?}", req.version());
                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                        http_body_util::Full::new(Bytes::from(body)),
                    ))
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(hyper_util::rt::TokioIo::new(tls), service)
                .await;
            });
        }
    });
    TlsServer {
        addr,
        ca_der: ca_cert.der().to_vec(),
    }
}

#[tokio::test]
async fn http2_is_negotiated_via_alpn() {
    let server = tls_server(&[b"h2", b"http/1.1"]).await;
    let t = NativeTransport::with_extra_root_certificates([server.ca_der]).unwrap();
    let url = format!("https://localhost:{}/", server.addr.port());
    let c = send(&t, request("GET", &url)).await.unwrap();
    assert_eq!(c.head.version, HttpVersion::Http2);
    assert_eq!(&c.body[..], b"HTTP/2.0");
    let timing = &c.head.timing;
    assert!(timing.dns.is_some(), "{timing:?}");
    assert!(timing.connect.is_some(), "{timing:?}");
    assert!(timing.tls.is_some(), "{timing:?}");

    // HTTP/2 multiplexes: the next request reuses the connection.
    let c = send(&t, request("GET", &url)).await.unwrap();
    assert_eq!(c.head.version, HttpVersion::Http2);
    assert_eq!(c.head.timing.tls, None);
}

#[tokio::test]
async fn http1_over_tls_when_server_has_no_h2() {
    let server = tls_server(&[b"http/1.1"]).await;
    let t = NativeTransport::with_extra_root_certificates([server.ca_der]).unwrap();
    let url = format!("https://127.0.0.1:{}/", server.addr.port());
    let c = send(&t, request("GET", &url)).await.unwrap();
    assert_eq!(c.head.version, HttpVersion::Http11);
    assert_eq!(&c.body[..], b"HTTP/1.1");
    assert!(c.head.timing.tls.is_some());
}

#[tokio::test]
async fn untrusted_certificate_is_a_tls_error() {
    let server = tls_server(&[b"h2", b"http/1.1"]).await;
    let t = NativeTransport::new().unwrap();
    let url = format!("https://localhost:{}/", server.addr.port());
    match t.send(request("GET", &url)).await {
        Err(TransportError::Tls(_)) => {}
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_tls_details_of_a_response_are_known_even_on_a_reused_connection() {
    let server = tls_server(&[b"h2", b"http/1.1"]).await;
    let t = NativeTransport::with_extra_root_certificates([server.ca_der]).unwrap();
    let url = format!("https://localhost:{}/", server.addr.port());
    assert_eq!(
        t.tls_info(&url),
        None,
        "nothing before the first connection"
    );
    send(&t, request("GET", &url)).await.unwrap();
    for _ in 0..2 {
        let info = t.tls_info(&format!("{url}other?x=1")).expect("tls info");
        assert_eq!(info.protocol.as_deref(), Some("TLSv1.3"));
        assert!(
            info.cipher
                .as_deref()
                .is_some_and(|c| c.starts_with("TLS13_"))
        );
        assert_eq!(info.key_algorithm.as_deref(), Some("EC"));
        assert_eq!(info.key_bits, Some(256));
        let now = crate::jwt::now_secs();
        assert!(info.not_before.unwrap() <= now + 86_400, "{info:?}");
        assert!(info.not_after.unwrap() > now, "{info:?}");
        // The next request reuses the connection (h2) and still has the details.
        send(&t, request("GET", &url)).await.unwrap();
    }
    // Plain http and other origins have none.
    assert_eq!(t.tls_info("http://localhost/"), None);
    assert_eq!(t.tls_info("https://localhost:1/"), None);
}
