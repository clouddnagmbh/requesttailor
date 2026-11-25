// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand E: `rt inbox` and the companion's `GET /v1/inbox`. Runs the built `rt` binary.
//! `rt inbox` listens on a port the OS picks (`--port 0`) and prints one line per request; the
//! companion runs on port 48606 and the inbox it starts on 48607 (the tests of the companion
//! share the ports, so they run one at a time).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use rt_core::inbox::{CapturedRequest, InboxEvent, InboxSettings};
use rt_core::transport::companion::{
    InboxClientMessage, InboxServerMessage, PairResponse, Status,
};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;

const CANARY: &str = "RT-CANARY-inbox-cli-31c8";
const AGENT_PORT: u16 = 48606;
const INBOX_PORT: u16 = 48607;
const ORIGIN: &str = "http://127.0.0.1:5201";

static LOCK: Mutex<()> = Mutex::new(());

fn lines_of(read: impl Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(read).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    rx
}

struct Inbox {
    child: Child,
    stdout: Receiver<String>,
    port: u16,
}

impl Inbox {
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .arg("inbox")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start rt inbox");
        let stdout = lines_of(child.stdout.take().unwrap());
        let stderr = lines_of(child.stderr.take().unwrap());
        let line = stderr
            .recv_timeout(Duration::from_secs(20))
            .expect("a line on stderr");
        let port = line
            .strip_prefix("Listening on http://127.0.0.1:")
            .and_then(|r| r.split('/').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(|| panic!("an address, got {line:?}"));
        Inbox { child, stdout, port }
    }

    fn line(&self) -> String {
        self.stdout
            .recv_timeout(Duration::from_secs(10))
            .expect("a line on stdout")
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn post(port: u16, target: &str, headers: &str, body: &str) -> String {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let req = format!(
        "POST {target} HTTP/1.1\r\nHost: x\r\n{headers}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn rt_inbox_prints_one_line_per_request_and_answers_with_the_fixed_reply() {
    let inbox = Inbox::start(&["--port", "0"]);
    let answer = post(
        inbox.port,
        "/hooks/orders?x=1",
        "Content-Type: application/json\r\n",
        "{\"id\":7}",
    );
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    let line = inbox.line();
    assert!(
        line.ends_with("POST /hooks/orders?x=1 (application/json, 8 B)"),
        "{line}"
    );
    // The line never carries headers or a body.
    post(inbox.port, "/s", &format!("Authorization: Bearer {CANARY}\r\n"), CANARY);
    assert!(!inbox.line().contains(CANARY));
}

#[test]
fn rt_inbox_json_prints_one_capture_per_line_and_uses_the_status() {
    let inbox = Inbox::start(&["--port", "0", "--status", "202", "--json"]);
    let answer = post(inbox.port, "/j", "X-Event: e\r\n", "body");
    assert!(answer.starts_with("HTTP/1.1 202 "), "{answer}");
    let request: CapturedRequest = serde_json::from_str(&inbox.line()).unwrap();
    assert_eq!((request.method.as_str(), request.target.as_str()), ("POST", "/j"));
    assert_eq!(request.body.as_deref(), Some("body"));
    assert!(request.headers.iter().any(|(n, v)| n == "X-Event" && v == "e"));
}

#[test]
fn rt_inbox_refuses_a_port_in_use_and_a_bad_status() {
    let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["inbox", "--port", &port])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("in use"));
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["inbox", "--port", "0", "--status", "99"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["inbox", "--host", "0.0.0.0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "there is no --host: loopback only");
}

#[test]
fn the_inbox_has_no_mcp_tool() {
    let mcp = include_str!("../src/mcp.rs").to_lowercase();
    assert!(!mcp.contains("inbox"), "agents never read the inbox");
}

// ---------------------------------------------------------------------------------------------
// The companion

struct Agent {
    child: Child,
    lines: Receiver<String>,
}

impl Agent {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .args(["agent", "--port", &AGENT_PORT.to_string(), "--allow-origin", ORIGIN])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start rt agent");
        let lines = lines_of(child.stdout.take().unwrap());
        Agent { child, lines }
    }

    fn pair(&self) -> String {
        let code = loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .expect("a pairing code");
            if let Some(rest) = line.strip_prefix("Pairing code: ") {
                break rest.split_whitespace().next().unwrap().to_owned();
            }
        };
        let body = serde_json::json!({ "code": code }).to_string();
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

fn http(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
    let mut stream = None;
    for _ in 0..100 {
        match TcpStream::connect(("127.0.0.1", AGENT_PORT)) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut stream = stream.expect("rt agent listens");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{AGENT_PORT}\r\nConnection: close\r\nContent-Length: {}\r\n",
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

async fn connect(query: &str, origin: &str) -> Result<Socket, u16> {
    let mut request = format!("ws://127.0.0.1:{AGENT_PORT}/v1/inbox{query}")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert("origin", origin.parse().unwrap());
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", AGENT_PORT)).await.unwrap();
    match tokio_tungstenite::client_async(request, tcp).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => Err(r.status().as_u16()),
        Err(e) => panic!("handshake: {e}"),
    }
}

async fn send(ws: &mut Socket, message: InboxClientMessage) {
    ws.send(Message::text(serde_json::to_string(&message).unwrap()))
        .await
        .unwrap();
}

async fn receive(ws: &mut Socket) -> InboxServerMessage {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a message within 5 s")
            .expect("the socket is open")
            .expect("no error");
        if let Message::Text(text) = message {
            return serde_json::from_str(text.as_str()).unwrap();
        }
    }
}

/// The next message that is not an event of `started`/`stopped`/`cleared`.
async fn status(ws: &mut Socket) -> rt_core::inbox::InboxStatus {
    loop {
        if let InboxServerMessage::Status { status } = receive(ws).await {
            return status;
        }
    }
}

async fn request_event(ws: &mut Socket) -> CapturedRequest {
    loop {
        if let InboxServerMessage::Event {
            event: InboxEvent::Request { request },
        } = receive(ws).await
        {
            return *request;
        }
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn the_companion_checks_the_token_and_the_origin_and_relays_the_inbox() {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let agent = Agent::start();
    let token = agent.pair();
    let (code, body) = http("GET", "/v1/status", &[("Origin", ORIGIN)], "");
    assert_eq!(code, 200);
    let features = serde_json::from_str::<Status>(&body).unwrap().features;
    assert!(features.contains(&"inbox".to_owned()), "{features:?}");

    runtime().block_on(async {
        // No token, a wrong token and a foreign origin are refused.
        assert_eq!(connect("", ORIGIN).await.err(), Some(401));
        assert_eq!(connect("?token=nope", ORIGIN).await.err(), Some(401));
        assert_eq!(
            connect(&format!("?token={token}"), "http://evil.example").await.err(),
            Some(403)
        );

        let mut ws = connect(&format!("?token={token}"), ORIGIN).await.unwrap();
        // It does not listen before it is started.
        let first = status(&mut ws).await;
        assert!(!first.running);
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, INBOX_PORT)).is_err());

        let settings = InboxSettings {
            port: INBOX_PORT,
            status: 201,
            ..InboxSettings::default()
        };
        send(&mut ws, InboxClientMessage::Start { settings }).await;
        let running = status(&mut ws).await;
        assert_eq!((running.running, running.port), (true, Some(INBOX_PORT)));

        let answer = post(
            INBOX_PORT,
            "/hook/a",
            &format!("X-Hub-Signature: {CANARY}\r\n"),
            "payload",
        );
        assert!(answer.starts_with("HTTP/1.1 201 "), "{answer}");
        let request = request_event(&mut ws).await;
        assert_eq!((request.method.as_str(), request.target.as_str()), ("POST", "/hook/a"));
        assert_eq!(request.body.as_deref(), Some("payload"));

        // A second page sees what is held.
        let mut other = connect(&format!("?token={token}"), ORIGIN).await.unwrap();
        assert_eq!(request_event(&mut other).await.id, request.id);
        drop(other);

        send(&mut ws, InboxClientMessage::Clear).await;
        assert_eq!(status(&mut ws).await.held, 0);

        // A port in use is a `failed`, not a crash.
        let _taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        send(&mut ws, InboxClientMessage::Stop).await;
        assert!(!status(&mut ws).await.running);
        let busy = TcpListener::bind((Ipv4Addr::LOCALHOST, INBOX_PORT)).unwrap();
        send(
            &mut ws,
            InboxClientMessage::Start {
                settings: InboxSettings {
                    port: INBOX_PORT,
                    ..InboxSettings::default()
                },
            },
        )
        .await;
        loop {
            if let InboxServerMessage::Failed { message } = receive(&mut ws).await {
                assert!(message.contains("in use"), "{message}");
                break;
            }
        }
        drop(busy);
    });
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, INBOX_PORT)).is_err(), "stopped");
}
