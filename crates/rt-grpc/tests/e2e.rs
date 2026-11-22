// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance (wave 8, strand B, points 2, 3 and 9): the native gRPC wire against the test
//! server: all four method types, error status, metadata, timeout, cancel, compression,
//! server reflection (v1, the v1alpha fallback, none). Native only.

#![cfg(not(target_arch = "wasm32"))]

use std::net::SocketAddr;

use rt_core::stream::{Frame, StreamEvent, StreamOpen, StreamSession, StreamTransport};
use rt_core::transport::TransportError;
use rt_core::transport::native::NativeTransport;
use rt_grpc::{Descriptors, GrpcError};
use rt_testserver::GrpcOptions;

const ECHO: &str = include_str!("../../../testdata/grpc/protos/echo.proto");
const COMMON: &str = include_str!("../../../testdata/grpc/include/rt/common.proto");
const SVC: &str = "/rt.echo.v1.EchoService";

fn descriptors() -> Descriptors {
    Descriptors::from_files(
        &vec![
            ("protos/echo.proto".into(), ECHO.into()),
            ("include/rt/common.proto".into(), COMMON.into()),
        ],
        &["protos/echo.proto".into()],
        &["include".into()],
    )
    .unwrap()
}

async fn server() -> SocketAddr {
    rt_testserver::spawn_grpc(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap()
}

fn grpc(addr: SocketAddr, method: &str, metadata: &[(&str, &str)]) -> StreamOpen {
    StreamOpen::Grpc {
        url: format!("http://{addr}"),
        path: format!("{SVC}/{method}"),
        metadata: metadata
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        timeout_ms: Some(10_000),
    }
}

/// Every remaining event of a session.
async fn drain(session: &mut Box<dyn StreamSession + '_>) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    while let Some(event) = session.next().await {
        events.push(event);
    }
    events
}

/// Received messages as JSON values.
fn replies(d: &Descriptors, method: &str, events: &[StreamEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Message {
                frame: Frame::Binary(b),
                ..
            } => Some(
                serde_json::from_str(&d.decode(&format!("{SVC}/{method}"), b).unwrap()).unwrap(),
            ),
            _ => None,
        })
        .collect()
}

fn closed(events: &[StreamEvent]) -> (Option<u32>, String, Vec<(String, String)>, bool) {
    match events.last() {
        Some(StreamEvent::Closed {
            code,
            reason,
            trailers,
            by_client,
            ..
        }) => (*code, reason.clone(), trailers.clone(), *by_client),
        other => panic!("not closed: {other:?}"),
    }
}

async fn send_json(session: &mut Box<dyn StreamSession + '_>, method: &str, json: &str) {
    let d = descriptors();
    let frame = d.encode_frame(&format!("{SVC}/{method}"), json).unwrap();
    session.send(frame).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn unary_with_metadata_headers_and_trailers() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let d = descriptors();
    let mut s = transport
        .open(grpc(
            addr,
            "Echo",
            &[("x-trace", "t-1"), ("X-Tenant", "acme")],
        ))
        .await
        .unwrap();
    send_json(&mut s, "Echo", r#"{"message": "hello"}"#).await;
    s.finish().await.unwrap();
    let events = drain(&mut s).await;
    let StreamEvent::Opened { head, .. } = &events[0] else {
        panic!("{events:?}");
    };
    assert_eq!(head.status, 200);
    assert_eq!(head.url, format!("http://{addr}{SVC}/Echo"));
    assert!(
        head.headers
            .contains(&("x-echo-header".into(), "t-1".into()))
    );
    let replies = replies(&d, "Echo", &events);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["message"], "hello");
    assert_eq!(replies[0]["metadata"]["entries"]["x-trace"], "t-1");
    // Metadata names go out in lowercase (HTTP/2).
    assert_eq!(replies[0]["metadata"]["entries"]["x-tenant"], "acme");
    let (code, _, trailers, by_client) = closed(&events);
    assert_eq!(code, Some(0));
    assert!(!by_client);
    assert_eq!(trailers, [("x-echo-trailer".to_owned(), "done".to_owned())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn server_streaming() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let mut s = transport.open(grpc(addr, "Countdown", &[])).await.unwrap();
    send_json(&mut s, "Countdown", r#"{"message": "tick", "count": 4}"#).await;
    s.finish().await.unwrap();
    let events = drain(&mut s).await;
    let replies = replies(&descriptors(), "Countdown", &events);
    let texts: Vec<&str> = replies
        .iter()
        .map(|r| r["message"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["tick 0", "tick 1", "tick 2", "tick 3"]);
    assert_eq!(closed(&events).0, Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn client_streaming_half_closes_on_finish() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let mut s = transport.open(grpc(addr, "Collect", &[])).await.unwrap();
    for word in ["a", "b", "c"] {
        send_json(&mut s, "Collect", &format!(r#"{{"message": "{word}"}}"#)).await;
    }
    s.finish().await.unwrap();
    assert!(
        s.send(Frame::Binary(Vec::new())).await.is_err(),
        "nothing after finish"
    );
    let events = drain(&mut s).await;
    let replies = replies(&descriptors(), "Collect", &events);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["message"], "a b c");
    assert_eq!(replies[0]["index"], 3);
    assert_eq!(closed(&events).0, Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn bidi_streaming_interleaves() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let d = descriptors();
    let mut s = transport.open(grpc(addr, "Chat", &[])).await.unwrap();
    assert!(matches!(s.next().await, Some(StreamEvent::Opened { .. })));
    for word in ["one", "two"] {
        send_json(&mut s, "Chat", &format!(r#"{{"message": "{word}"}}"#)).await;
        // Each reply arrives before the next message goes out.
        let event = s.next().await.unwrap();
        let reply = replies(&d, "Chat", std::slice::from_ref(&event));
        assert_eq!(reply[0]["message"], word, "{event:?}");
    }
    s.finish().await.unwrap();
    let rest = drain(&mut s).await;
    assert_eq!(closed(&rest).0, Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_error_status_comes_trailers_only() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let mut s = transport.open(grpc(addr, "Fail", &[])).await.unwrap();
    send_json(
        &mut s,
        "Fail",
        r#"{"code": 7, "message": "no access: ä 100%"}"#,
    )
    .await;
    s.finish().await.unwrap();
    let events = drain(&mut s).await;
    assert!(matches!(events[0], StreamEvent::Opened { .. }));
    assert_eq!(events.len(), 2, "{events:?}");
    let (code, reason, _, _) = closed(&events);
    assert_eq!(code, Some(7));
    assert_eq!(reason, "no access: ä 100%");

    let mut s = transport.open(grpc(addr, "Nope", &[])).await.unwrap();
    s.finish().await.unwrap();
    assert_eq!(closed(&drain(&mut s).await).0, Some(12));
}

#[tokio::test(flavor = "multi_thread")]
async fn compressed_responses_are_refused_clearly() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let mut s = transport
        .open(grpc(addr, "Echo", &[("x-compress", "1")]))
        .await
        .unwrap();
    send_json(&mut s, "Echo", r#"{"message": "zip"}"#).await;
    s.finish().await.unwrap();
    let events = drain(&mut s).await;
    let Some(StreamEvent::Error {
        error: TransportError::Other(message),
        ..
    }) = events.last()
    else {
        panic!("{events:?}");
    };
    assert!(message.contains("compressed"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_timeout_cancels_with_deadline_exceeded() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let StreamOpen::Grpc { url, path, .. } = grpc(addr, "Countdown", &[]) else {
        unreachable!()
    };
    let mut s = transport
        .open(StreamOpen::Grpc {
            url,
            path,
            metadata: Vec::new(),
            timeout_ms: Some(300),
        })
        .await
        .unwrap();
    send_json(
        &mut s,
        "Countdown",
        r#"{"message": "slow", "count": 5, "delayMs": 1000}"#,
    )
    .await;
    s.finish().await.unwrap();
    let started = std::time::Instant::now();
    let events = drain(&mut s).await;
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    let (code, _, _, _) = closed(&events);
    assert_eq!(code, Some(4), "{events:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn close_cancels_the_call() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let mut s = transport.open(grpc(addr, "Chat", &[])).await.unwrap();
    send_json(&mut s, "Chat", r#"{"message": "hi"}"#).await;
    assert!(matches!(s.next().await, Some(StreamEvent::Opened { .. })));
    assert!(matches!(s.next().await, Some(StreamEvent::Message { .. })));
    s.close().await;
    let events = drain(&mut s).await;
    let (code, _, _, by_client) = closed(&events);
    assert_eq!(code, Some(1));
    assert!(by_client);
    assert!(s.send(Frame::Binary(Vec::new())).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn not_a_grpc_server_is_reported() {
    // The HTTP test server answers HTTP/2 with JSON.
    let http = rt_testserver::spawn_local().await;
    let transport = NativeTransport::new().unwrap();
    let mut s = transport
        .open(StreamOpen::Grpc {
            url: format!("http://{http}"),
            path: "/echo".into(),
            metadata: Vec::new(),
            timeout_ms: None,
        })
        .await
        .unwrap();
    s.finish().await.unwrap();
    let events = drain(&mut s).await;
    assert!(
        matches!(events.last(), Some(StreamEvent::Error { error: TransportError::Other(m), .. }) if m.contains("not a gRPC response")),
        "{events:?}"
    );
    // Nobody listening.
    let free = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let err = transport
        .open(grpc(free, "Echo", &[]))
        .await
        .err()
        .expect("connect fails");
    assert!(matches!(err, TransportError::Connect(_)), "{err:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reflection_lists_the_same_services_as_the_proto_files() {
    let addr = server().await;
    let transport = NativeTransport::new().unwrap();
    let reflected = Descriptors::from_reflection(&transport, &format!("grpc://{addr}"), &[])
        .await
        .unwrap();
    assert_eq!(reflected.services(), descriptors().services());
    // Imports (rt/common.proto, the well-known Timestamp) came along.
    let template = reflected.template(&format!("{SVC}/Echo")).unwrap();
    assert!(
        template.contains("\"sentAt\": \"1970-01-01T00:00:00Z\""),
        "{template}"
    );
    assert!(template.contains("PRIORITY_UNSPECIFIED"), "{template}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reflection_falls_back_to_v1alpha_and_reports_none() {
    let transport = NativeTransport::new().unwrap();
    let alpha = rt_testserver::spawn_grpc_with(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        GrpcOptions {
            reflection_v1: false,
            reflection_v1alpha: true,
        },
    )
    .await
    .unwrap();
    let d = Descriptors::from_reflection(&transport, &format!("{alpha}"), &[])
        .await
        .unwrap();
    assert_eq!(d.services().len(), 1);

    let none = rt_testserver::spawn_grpc_with(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        GrpcOptions {
            reflection_v1: false,
            reflection_v1alpha: false,
        },
    )
    .await
    .unwrap();
    let err = Descriptors::from_reflection(&transport, &format!("grpc://{none}"), &[])
        .await
        .unwrap_err();
    assert!(
        matches!(&err, GrpcError::Reflection(m) if m.contains("does not implement")),
        "{err:?}"
    );
}
