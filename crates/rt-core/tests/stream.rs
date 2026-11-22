// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sessions (wave 8, strand A): `rt_core::stream::open` over the native transport against
//! rt-testserver's WebSocket and SSE routes, masking of the history, keep-alive, timeouts,
//! close codes, and `wss://` with a local CA.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rt_core::model::{
    Auth, Collection, Environment, Item, KeyValue, MessageKind, MessageVariant, Protocol,
    RequestEntry, RequestSpec, Script, ScriptPhase, VarScope, Variable, WebSocketSpec,
};
use rt_core::prepare::{FileSource, PrepareInput};
use rt_core::script::{ScriptCall, ScriptOutcome, VarChange};
use rt_core::send::{SendError, SendOptions};
use rt_core::services::{LocalBoxFuture, ScriptRunner, Services};
use rt_core::stream::{Command, Direction, Frame, Session, StreamEvent, open};
use rt_core::transport::TransportError;
use rt_core::transport::native::NativeTransport;

const SECRET: &str = "s3cr3t-value";

struct NoFiles;

impl FileSource for NoFiles {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        Err(format!("{path}: not found"))
    }
}

fn header(name: &str, value: &str) -> KeyValue {
    KeyValue {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn variant(title: &str, selected: bool, kind: MessageKind, data: &str) -> MessageVariant {
    MessageVariant {
        title: title.into(),
        selected,
        kind,
        data: data.into(),
    }
}

fn ws_spec(url: String, headers: Vec<KeyValue>, ws: WebSocketSpec) -> RequestSpec {
    RequestSpec {
        name: "socket".into(),
        method: "WS".into(),
        url,
        headers,
        protocol: Protocol::WebSocket(Box::new(ws)),
        ..Default::default()
    }
}

fn collection(spec: RequestSpec) -> Collection {
    Collection {
        name: "c".into(),
        defaults: Default::default(),
        items: vec![Item::Request(RequestEntry {
            path: "r.yml".into(),
            spec,
        })],
        environments: vec![Environment {
            name: "E".into(),
            variables: vec![Variable {
                name: "key".into(),
                value: Some(SECRET.into()),
                enabled: true,
                secret: true,
            }],
            client_certificates: Vec::new(),
            external_secrets: None,
        }],
        config: Default::default(),
    }
}

fn input() -> PrepareInput<'static> {
    PrepareInput {
        environment: Some("E"),
        globals: Vec::new(),
        runtime: Vec::new(),
    }
}

async fn open_with<'a>(
    c: &Collection,
    transport: &'a NativeTransport,
    services: &Services<'_>,
) -> Result<Session<'a>, SendError> {
    let context = c.request("r.yml").expect("request");
    open(
        &context,
        &input(),
        &NoFiles,
        &SendOptions::default(),
        services,
        transport,
        transport,
    )
    .await
}

async fn next(session: &mut Session<'_>) -> StreamEvent {
    tokio::time::timeout(Duration::from_secs(5), session.next())
        .await
        .expect("an event within 5 s")
        .expect("the session is not over")
}

fn received_text(event: &StreamEvent) -> &str {
    match event {
        StreamEvent::Message {
            direction: Direction::Received,
            frame: Frame::Text(text),
            ..
        } => text,
        other => panic!("expected a received text message, got {other:?}"),
    }
}

fn assert_no_secret(session: &Session<'_>) {
    let json = serde_json::to_string(&session.log).unwrap();
    assert!(!json.contains(SECRET), "secret in the log: {json}");
    for event in &session.log.events {
        if let StreamEvent::Message {
            frame: Frame::Binary(bytes),
            ..
        } = event
        {
            assert!(
                !bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()),
                "secret in a binary frame"
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_echo_text_binary_close_and_masking() {
    let addr = rt_testserver::spawn_local().await;
    let c = collection(ws_spec(
        format!("ws://{addr}/ws/echo?token={{{{key}}}}"),
        vec![header("X-Key", "{{key}}")],
        WebSocketSpec {
            messages: vec![variant("hello", true, MessageKind::Text, "hi {{key}}")],
            ..Default::default()
        },
    ));
    let transport = NativeTransport::new().unwrap();
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    assert_eq!(session.log.kind, "websocket");
    assert!(session.log.url.contains("token=***"), "{}", session.log.url);

    let opened = next(&mut session).await;
    let StreamEvent::Opened { head, .. } = &opened else {
        panic!("expected opened, got {opened:?}");
    };
    assert_eq!(head.status, 101);
    assert!(!head.url.contains(SECRET));

    let message = rt_core::stream::selected_message(match &c.items[0] {
        Item::Request(r) => match &r.spec.protocol {
            Protocol::WebSocket(ws) => &ws.messages,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    })
    .unwrap()
    .clone();
    let frame = session.frame(&message).unwrap();
    assert_eq!(frame, Frame::Text(format!("hi {SECRET}")));
    session.send(frame).await.unwrap();
    let echo = next(&mut session).await;
    // The peer echoed the secret; what we hand out is masked.
    assert_eq!(received_text(&echo), "hi ***");

    let mut bytes = b"\x00\x01".to_vec();
    bytes.extend_from_slice(SECRET.as_bytes());
    session.send(Frame::Binary(bytes)).await.unwrap();
    match next(&mut session).await {
        StreamEvent::Message {
            frame: Frame::Binary(b),
            direction: Direction::Received,
            ..
        } => assert_eq!(b, b"\x00\x01***".to_vec()),
        other => panic!("expected binary echo, got {other:?}"),
    }

    session.close().await;
    match next(&mut session).await {
        StreamEvent::Closed {
            code, by_client, ..
        } => {
            assert_eq!(code, Some(1000));
            assert!(by_client);
        }
        other => panic!("expected closed, got {other:?}"),
    }
    assert!(session.next().await.is_none());
    let directions: Vec<_> = session
        .log
        .events
        .iter()
        .map(|e| match e {
            StreamEvent::Opened { .. } => "opened",
            StreamEvent::Message {
                direction: Direction::Sent,
                ..
            } => "sent",
            StreamEvent::Message { .. } => "received",
            StreamEvent::Closed { .. } => "closed",
            StreamEvent::Error { .. } => "error",
        })
        .collect();
    assert_eq!(
        directions,
        ["opened", "sent", "received", "sent", "received", "closed"]
    );
    assert_no_secret(&session);
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_handshake_headers_protocols_and_auth() {
    let addr = rt_testserver::spawn_local().await;
    let mut spec = ws_spec(
        format!("ws://{addr}/ws/headers"),
        vec![
            header("X-Trace", "abc"),
            header("Sec-WebSocket-Protocol", "chat, v2"),
        ],
        WebSocketSpec::default(),
    );
    spec.auth = Auth::Bearer {
        token: "{{key}}".into(),
    };
    let c = collection(spec);
    let transport = NativeTransport::new().unwrap();
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    let StreamEvent::Opened { head, .. } = next(&mut session).await else {
        panic!("expected opened");
    };
    let protocol = head
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-protocol"))
        .map(|(_, v)| v.as_str());
    assert_eq!(protocol, Some("chat"));
    let event = next(&mut session).await;
    let headers: Vec<(String, String)> = serde_json::from_str(received_text(&event)).unwrap();
    let get = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("x-trace"), Some("abc"));
    assert_eq!(get("sec-websocket-protocol"), Some("chat, v2"));
    // The bearer token went on the wire; the log has it masked.
    assert_eq!(get("authorization"), Some("Bearer ***"));
    match next(&mut session).await {
        StreamEvent::Closed {
            code,
            reason,
            by_client,
            ..
        } => {
            assert_eq!(
                (code, reason.as_str(), by_client),
                (Some(1000), "done", false)
            );
        }
        other => panic!("expected closed, got {other:?}"),
    }
    assert!(session.next().await.is_none());
    assert_no_secret(&session);
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_server_close_code_and_reason() {
    let addr = rt_testserver::spawn_local().await;
    let c = collection(ws_spec(
        format!("ws://{addr}/ws/echo?close=4001&after=0"),
        Vec::new(),
        WebSocketSpec::default(),
    ));
    let transport = NativeTransport::new().unwrap();
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    assert!(matches!(
        next(&mut session).await,
        StreamEvent::Opened { .. }
    ));
    match next(&mut session).await {
        StreamEvent::Closed {
            code,
            reason,
            by_client,
            ..
        } => assert_eq!(
            (code, reason.as_str(), by_client),
            (Some(4001), "closed by request", false)
        ),
        other => panic!("expected closed, got {other:?}"),
    }
    // A closed session refuses to send.
    assert!(session.send(Frame::Text("x".into())).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_connect_timeout_and_refused_handshake() {
    // A server that accepts TCP and never answers the handshake.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent_addr = silent.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = silent.accept().await {
            held.push(stream);
        }
    });
    let c = collection(ws_spec(
        format!("ws://{silent_addr}/"),
        Vec::new(),
        WebSocketSpec {
            timeout_ms: Some(200),
            ..Default::default()
        },
    ));
    let transport = NativeTransport::new().unwrap();
    let started = std::time::Instant::now();
    let error = open_with(&c, &transport, &Services::none())
        .await
        .err()
        .expect("times out");
    assert_eq!(error, SendError::Transport(TransportError::Timeout));
    assert!(started.elapsed() < Duration::from_secs(3));

    // An HTTP route that does not upgrade.
    let addr = rt_testserver::spawn_local().await;
    let c = collection(ws_spec(
        format!("ws://{addr}/status/403?k={{{{key}}}}"),
        Vec::new(),
        WebSocketSpec::default(),
    ));
    let error = open_with(&c, &transport, &Services::none())
        .await
        .err()
        .expect("refused");
    let SendError::Transport(TransportError::Other(message)) = error else {
        panic!("unexpected error {error:?}");
    };
    assert!(message.contains("HTTP 403"), "{message}");

    // Not a WebSocket URL.
    let c = collection(ws_spec(
        format!("http://{addr}/ws/echo"),
        Vec::new(),
        WebSocketSpec::default(),
    ));
    assert!(matches!(
        open_with(&c, &transport, &Services::none()).await.err(),
        Some(SendError::Transport(TransportError::InvalidUrl(_)))
    ));
}

/// A WebSocket server that counts the pings it receives.
async fn ping_counter() -> (SocketAddr, Arc<std::sync::atomic::AtomicUsize>) {
    use futures_util::StreamExt as _;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let pings = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = pings.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        while let Some(Ok(message)) = ws.next().await {
            if message.is_ping() {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    });
    (addr, pings)
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_keep_alive_pings() {
    let (addr, pings) = ping_counter().await;
    let c = collection(ws_spec(
        format!("ws://{addr}/"),
        Vec::new(),
        WebSocketSpec {
            keep_alive_ms: Some(50),
            ..Default::default()
        },
    ));
    let transport = NativeTransport::new().unwrap();
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    assert!(matches!(
        next(&mut session).await,
        StreamEvent::Opened { .. }
    ));
    // Nothing arrives; `next` keeps sending pings while it waits.
    let waited = tokio::time::timeout(Duration::from_millis(400), session.next()).await;
    assert!(waited.is_err(), "no event expected");
    let count = pings.load(std::sync::atomic::Ordering::SeqCst);
    assert!(count >= 3, "only {count} pings");
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_drive_runs_commands_and_reports_every_event() {
    let addr = rt_testserver::spawn_local().await;
    let c = collection(ws_spec(
        format!("ws://{addr}/ws/echo"),
        vec![header("X-Key", "{{key}}")],
        WebSocketSpec::default(),
    ));
    let transport = NativeTransport::new().unwrap();
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let seen = RefCell::new(Vec::new());
    tx.send(Command::Send(Frame::Text(format!("one {SECRET}"))))
        .unwrap();
    tx.send(Command::Send(Frame::Text("two".into()))).unwrap();
    let driver = session.drive(
        |cx| rx.poll_recv(cx),
        |event| {
            seen.borrow_mut().push(event.clone());
            // Close after both echoes arrived.
            let echoes = seen
                .borrow()
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        StreamEvent::Message {
                            direction: Direction::Received,
                            ..
                        }
                    )
                })
                .count();
            if echoes == 2 {
                let _ = tx.send(Command::Close);
            }
        },
    );
    tokio::time::timeout(Duration::from_secs(5), driver)
        .await
        .expect("the session ends");
    let seen = seen.into_inner();
    let texts: Vec<String> = seen
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Message {
                direction,
                frame: Frame::Text(t),
                ..
            } => Some(format!("{direction:?}:{t}")),
            _ => None,
        })
        .collect();
    // Echoes may arrive between the two sends; each message is there once, masked.
    let mut sorted = texts.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "Received:one ***",
            "Received:two",
            "Sent:one ***",
            "Sent:two"
        ]
    );
    assert!(matches!(
        seen.last(),
        Some(StreamEvent::Closed {
            by_client: true,
            ..
        })
    ));
    assert_eq!(session.log.events, seen);
    assert_no_secret(&session);
}

fn sse_spec(url: String) -> RequestSpec {
    RequestSpec {
        name: "events".into(),
        method: "GET".into(),
        url,
        ..Default::default()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn sse_count_and_named_events() {
    let addr = rt_testserver::spawn_local().await;
    let transport = NativeTransport::new().unwrap();

    let c = collection(sse_spec(format!("http://{addr}/sse/count/3?k={{{{key}}}}")));
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    assert_eq!(session.log.kind, "sse");
    let StreamEvent::Opened { head, .. } = next(&mut session).await else {
        panic!("expected opened");
    };
    assert_eq!(head.status, 200);
    for i in 1..=3 {
        match next(&mut session).await {
            StreamEvent::Message {
                frame: Frame::Text(data),
                id,
                name,
                direction: Direction::Received,
                ..
            } => {
                assert_eq!(data, i.to_string());
                assert_eq!(id, Some(i.to_string()));
                assert_eq!(name, None);
            }
            other => panic!("expected event {i}, got {other:?}"),
        }
    }
    assert!(matches!(
        next(&mut session).await,
        StreamEvent::Closed {
            code: None,
            by_client: false,
            ..
        }
    ));
    assert!(session.next().await.is_none());
    assert_no_secret(&session);
    // An SSE session only receives.
    assert!(session.send(Frame::Text("x".into())).await.is_err());

    let c = collection(sse_spec(format!("http://{addr}/sse/named")));
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    next(&mut session).await;
    let mut got = Vec::new();
    loop {
        match next(&mut session).await {
            StreamEvent::Message {
                frame: Frame::Text(data),
                name,
                id,
                ..
            } => got.push((name, id, data)),
            StreamEvent::Closed { .. } => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    let s = |v: &str| Some(v.to_owned());
    assert_eq!(
        got,
        vec![
            (s("greeting"), s("1"), "hello".to_owned()),
            (s("update"), s("2"), "{\"n\":2}".to_owned()),
            (s("update"), s("3"), "line one\nline two".to_owned()),
            (None, s("4"), "default type".to_owned()),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn sse_close_by_client_and_not_an_event_stream() {
    let addr = rt_testserver::spawn_local().await;
    let transport = NativeTransport::new().unwrap();
    let c = collection(sse_spec(format!(
        "http://{addr}/sse/count/1000?interval=50"
    )));
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    next(&mut session).await;
    received_text(&next(&mut session).await);
    session.close().await;
    assert!(matches!(
        next(&mut session).await,
        StreamEvent::Closed {
            by_client: true,
            ..
        }
    ));
    assert!(session.next().await.is_none());

    let c = collection(sse_spec(format!("http://{addr}/status/401")));
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    let StreamEvent::Opened { head, .. } = next(&mut session).await else {
        panic!("expected opened");
    };
    assert_eq!(head.status, 401);
    assert!(received_text(&next(&mut session).await).contains("401"));
    match next(&mut session).await {
        StreamEvent::Error {
            error: TransportError::Other(m),
            ..
        } => assert!(m.contains("not an event stream"), "{m}"),
        other => panic!("expected error, got {other:?}"),
    }
    assert!(session.next().await.is_none());
}

/// Sets `path` and adds a header, like a `before-request` script.
struct BeforeRequest;

impl ScriptRunner for BeforeRequest {
    fn run<'a>(&'a self, call: ScriptCall<'a>) -> LocalBoxFuture<'a, ScriptOutcome> {
        Box::pin(async move {
            assert!(call.sender.is_none());
            let mut request = call.request.clone();
            request.headers.push(("X-From-Script".into(), "yes".into()));
            ScriptOutcome {
                request: Some(request),
                var_changes: vec![VarChange {
                    name: "route".into(),
                    value: Some("headers".into()),
                    scope: VarScope::Runtime,
                }],
                ..Default::default()
            }
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn before_request_scripts_run_as_for_http() {
    let addr = rt_testserver::spawn_local().await;
    let mut spec = ws_spec(
        format!("ws://{addr}/ws/{{{{route}}}}"),
        Vec::new(),
        WebSocketSpec::default(),
    );
    spec.scripts = vec![Script {
        phase: ScriptPhase::BeforeRequest,
        code: "/* fake */".into(),
    }];
    let c = collection(spec);
    let transport = NativeTransport::new().unwrap();
    let runner = BeforeRequest;
    let services = Services {
        scripts: Some(&runner),
        ..Services::none()
    };
    let mut session = open_with(&c, &transport, &services).await.unwrap();
    next(&mut session).await;
    let event = next(&mut session).await;
    let headers: Vec<(String, String)> = serde_json::from_str(received_text(&event)).unwrap();
    assert!(
        headers
            .iter()
            .any(|(k, v)| k == "x-from-script" && v == "yes")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn wss_with_a_local_ca() {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair};
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "rt test CA");
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["localhost".to_owned()])
        .unwrap()
        .signed_by(&leaf_key, &issuer)
        .unwrap();

    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into()),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                use futures_util::{SinkExt as _, StreamExt as _};
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut ws = tokio_tungstenite::accept_async(tls).await.unwrap();
                while let Some(Ok(message)) = ws.next().await {
                    if message.is_text() && ws.send(message).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let c = collection(ws_spec(
        format!("wss://localhost:{port}/"),
        Vec::new(),
        WebSocketSpec::default(),
    ));
    let transport =
        NativeTransport::with_extra_root_certificates([ca_cert.der().to_vec()]).unwrap();
    let mut session = open_with(&c, &transport, &Services::none()).await.unwrap();
    assert!(matches!(
        next(&mut session).await,
        StreamEvent::Opened { .. }
    ));
    session.send(Frame::Text("over TLS".into())).await.unwrap();
    assert_eq!(received_text(&next(&mut session).await), "over TLS");

    // Without the CA the handshake fails with a TLS error.
    let plain = NativeTransport::new().unwrap();
    assert!(matches!(
        open_with(&c, &plain, &Services::none()).await.err(),
        Some(SendError::Transport(TransportError::Tls(_)))
    ));
}
