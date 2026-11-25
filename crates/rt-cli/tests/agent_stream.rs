// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt agent`'s session relay `GET /v1/stream` (wave 8, strand A): runs the built `rt` binary
//! on port 47881 next to rt-testserver and speaks the companion's stream protocol as the
//! browser would (WebSocket with `Origin`, token in the query).
//!
//! The tests share the port, so they run one at a time (`LOCK`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use rt_core::stream::{Direction, Frame, StreamEvent};
use rt_core::transport::TransportError;
use rt_core::transport::companion::{
    PairResponse, Status, StreamClientMessage, StreamServerMessage, WireRequest, WireStreamOpen,
};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;

const PORT: u16 = 47881;
const ORIGIN: &str = "http://127.0.0.1:5201";

static LOCK: Mutex<()> = Mutex::new(());

fn testserver() -> SocketAddr {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            tx.send(rt_testserver::spawn_local().await).unwrap();
            std::future::pending::<()>().await;
        });
    });
    rx.recv().unwrap()
}

struct Agent {
    child: Child,
    lines: Receiver<String>,
}

impl Agent {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .args([
                "agent",
                "--port",
                &PORT.to_string(),
                "--allow-origin",
                ORIGIN,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start rt agent");
        let (tx, lines) = channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        Agent { child, lines }
    }

    fn code(&self) -> String {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .expect("a pairing code from rt agent");
            if let Some(rest) = line.strip_prefix("Pairing code: ") {
                return rest.split_whitespace().next().unwrap().to_owned();
            }
        }
    }

    fn pair(&self) -> String {
        let body = serde_json::json!({ "code": self.code() }).to_string();
        let (status, body) = http(
            "POST",
            "/v1/pair",
            &[("Origin", ORIGIN), ("Content-Type", "application/json")],
            &body,
        );
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<PairResponse>(&body).unwrap().token
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One raw HTTP/1.1 request to the companion: status and body.
fn http(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
    let mut stream = None;
    for _ in 0..100 {
        match TcpStream::connect(("127.0.0.1", PORT)) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut stream = stream.expect("rt agent listens");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
}

type Socket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

/// Opens `/v1/stream` as a browser would; the handshake error (status) if refused.
async fn connect(query: &str, origin: &str) -> Result<Socket, u16> {
    let mut request = format!("ws://127.0.0.1:{PORT}/v1/stream{query}")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", PORT))
        .await
        .unwrap();
    match tokio_tungstenite::client_async(request, tcp).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => Err(r.status().as_u16()),
        Err(e) => panic!("handshake: {e}"),
    }
}

async fn send(ws: &mut Socket, message: StreamClientMessage) {
    ws.send(Message::text(serde_json::to_string(&message).unwrap()))
        .await
        .unwrap();
}

async fn receive(ws: &mut Socket) -> Option<StreamServerMessage> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a message within 5 s")?;
        match message {
            Ok(Message::Text(text)) => return Some(serde_json::from_str(text.as_str()).unwrap()),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue,
        }
    }
}

async fn event(ws: &mut Socket) -> StreamEvent {
    match receive(ws).await {
        Some(StreamServerMessage::Event { event }) => event,
        other => panic!("expected an event, got {other:?}"),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn websocket_and_sse_through_the_companion() {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let addr = testserver();
    let agent = Agent::start();
    let token = agent.pair();

    let (status, body) = http("GET", "/v1/status", &[("Origin", ORIGIN)], "");
    assert_eq!(status, 200);
    let status: Status = serde_json::from_str(&body).unwrap();
    assert!(status.features.contains(&"stream".to_owned()));

    runtime().block_on(async {
        let mut ws = connect(&format!("?token={token}"), ORIGIN).await.unwrap();
        send(
            &mut ws,
            StreamClientMessage::Open {
                open: WireStreamOpen::WebSocket {
                    url: format!("ws://{addr}/ws/echo"),
                    headers: vec![("X-Test".into(), "1".into())],
                    protocols: Vec::new(),
                    timeout_ms: Some(5000),
                    keep_alive_ms: None,
                },
            },
        )
        .await;
        assert!(
            matches!(event(&mut ws).await, StreamEvent::Opened { head, .. } if head.status == 101)
        );
        send(
            &mut ws,
            StreamClientMessage::Send {
                frame: Frame::Text("through the companion".into()),
            },
        )
        .await;
        match event(&mut ws).await {
            StreamEvent::Message {
                direction: Direction::Received,
                frame: Frame::Text(text),
                ..
            } => assert_eq!(text, "through the companion"),
            other => panic!("expected the echo, got {other:?}"),
        }
        send(&mut ws, StreamClientMessage::Close).await;
        assert!(matches!(
            event(&mut ws).await,
            StreamEvent::Closed {
                by_client: true,
                ..
            }
        ));
        assert!(receive(&mut ws).await.is_none(), "the relay closes");

        let mut ws = connect(&format!("?token={token}"), ORIGIN).await.unwrap();
        let request = WireRequest {
            method: "GET".into(),
            url: format!("http://{addr}/sse/count/2"),
            headers: vec![("Accept".into(), "text/event-stream".into())],
            body: None,
            content_type: None,
            timeout_ms: None,
            follow_redirects: true,
            max_redirects: 5,
        };
        send(
            &mut ws,
            StreamClientMessage::Open {
                open: WireStreamOpen::Sse { request },
            },
        )
        .await;
        assert!(matches!(event(&mut ws).await, StreamEvent::Opened { .. }));
        for i in 1..=2 {
            match event(&mut ws).await {
                StreamEvent::Message {
                    frame: Frame::Text(data),
                    id,
                    ..
                } => assert_eq!((data, id), (i.to_string(), Some(i.to_string()))),
                other => panic!("expected event {i}, got {other:?}"),
            }
        }
        assert!(matches!(
            event(&mut ws).await,
            StreamEvent::Closed {
                by_client: false,
                ..
            }
        ));
    });
}

#[test]
fn the_relay_checks_token_origin_and_target() {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let agent = Agent::start();
    let token = agent.pair();
    runtime().block_on(async {
        assert_eq!(connect("", ORIGIN).await.err(), Some(401));
        assert_eq!(connect("?token=wrong", ORIGIN).await.err(), Some(401));
        assert_eq!(
            connect(&format!("?token={token}"), "https://evil.example")
                .await
                .err(),
            Some(403)
        );

        // Not to the companion itself.
        let mut ws = connect(&format!("?token={token}"), ORIGIN).await.unwrap();
        send(
            &mut ws,
            StreamClientMessage::Open {
                open: WireStreamOpen::WebSocket {
                    url: format!("ws://127.0.0.1:{PORT}/v1/stream"),
                    headers: Vec::new(),
                    protocols: Vec::new(),
                    timeout_ms: None,
                    keep_alive_ms: None,
                },
            },
        )
        .await;
        assert!(matches!(
            receive(&mut ws).await,
            Some(StreamServerMessage::Failed {
                error: TransportError::Blocked(_)
            })
        ));

        // A failed open is answered with `failed`.
        let mut ws = connect(&format!("?token={token}"), ORIGIN).await.unwrap();
        send(
            &mut ws,
            StreamClientMessage::Open {
                open: WireStreamOpen::WebSocket {
                    url: "ws://127.0.0.1:9/".into(),
                    headers: Vec::new(),
                    protocols: Vec::new(),
                    // Windows retries a refused connection for about 2 s before it reports it.
                    timeout_ms: Some(15_000),
                    keep_alive_ms: None,
                },
            },
        )
        .await;
        assert!(matches!(
            receive(&mut ws).await,
            Some(StreamServerMessage::Failed {
                error: TransportError::Connect(_)
            })
        ));

        // The first message must be `open`.
        let mut ws = connect(&format!("?token={token}"), ORIGIN).await.unwrap();
        send(&mut ws, StreamClientMessage::Close).await;
        assert!(matches!(
            receive(&mut ws).await,
            Some(StreamServerMessage::Failed { .. })
        ));
    });
}
