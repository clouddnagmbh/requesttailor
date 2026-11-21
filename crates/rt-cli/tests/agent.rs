// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt agent` as a security boundary: runs the built `rt` binary on port 47822 next to
//! rt-testserver and checks each rule from the README's "Companion" section with raw HTTP
//! requests (so Host, Origin and preflight headers are exactly what the test says).
//!
//! The tests share the port, so they run one at a time (`AGENT_LOCK`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rt_core::transport::TransportError;
use rt_core::transport::companion::{
    HEAD_HEADER, PROTOCOL_VERSION, PairResponse, Status, WireRequest, decode_head,
};

const PORT: u16 = 47822;
const ORIGIN: &str = "http://127.0.0.1:5187";
const OTHER_ORIGIN: &str = "http://localhost:5187";
const EVIL_ORIGIN: &str = "https://evil.example";

static AGENT_LOCK: Mutex<()> = Mutex::new(());

/// rt-testserver on a random port, for the whole test process.
fn testserver() -> SocketAddr {
    static ADDR: OnceLock<SocketAddr> = OnceLock::new();
    *ADDR.get_or_init(|| {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                tx.send(rt_testserver::spawn_local().await).unwrap();
                std::future::pending::<()>().await;
            });
        });
        rx.recv().unwrap()
    })
}

struct Agent {
    child: Child,
    lines: Receiver<String>,
    stderr: Arc<Mutex<String>>,
    /// Stdout lines read so far.
    seen: Vec<String>,
    code: String,
    _lock: MutexGuard<'static, ()>,
}

impl Agent {
    fn start() -> Self {
        let lock = AGENT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .args(["agent", "--port", &PORT.to_string()])
            .args(["--allow-origin", ORIGIN, "--allow-origin", OTHER_ORIGIN])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start rt agent");
        let (tx, lines) = channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        let stderr = Arc::new(Mutex::new(String::new()));
        let sink = stderr.clone();
        let mut err = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });
        let mut agent = Agent {
            child,
            lines,
            stderr,
            seen: Vec::new(),
            code: String::new(),
            _lock: lock,
        };
        agent.code = agent.next_code();
        agent
    }

    /// Waits for the next `Pairing code: ` line on stdout.
    fn next_code(&mut self) -> String {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .unwrap_or_else(|_| {
                    panic!(
                        "no pairing code from rt agent; stderr: {}",
                        self.stderr.lock().unwrap()
                    )
                });
            self.seen.push(line.clone());
            if let Some(rest) = line.strip_prefix("Pairing code: ") {
                return rest.split_whitespace().next().unwrap().to_owned();
            }
        }
    }

    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    /// Pairs `origin` with the current code and returns the token.
    fn pair(&mut self, origin: &str) -> String {
        let code = self.code.clone();
        let r = post(PAIR, origin, None, &serde_json::json!({ "code": code }));
        assert_eq!(r.status, 200, "{}", r.text());
        self.code = self.next_code();
        let token = serde_json::from_slice::<PairResponse>(&r.body)
            .unwrap()
            .token;
        assert!(token.len() >= 43, "256-bit token: {token}");
        token
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const STATUS: &str = "/v1/status";
const PAIR: &str = "/v1/pair";
const SEND: &str = "/v1/send";

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("{e}: {}", self.text()))
    }
}

/// One HTTP/1.1 request on a fresh connection. `headers` are sent as given; a `Host` header
/// is added unless `headers` has one.
fn http(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
    let mut stream = TcpStream::connect(("127.0.0.1", PORT)).expect("connect to rt agent");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("host")) {
        head.push_str(&format!("Host: 127.0.0.1:{PORT}\r\n"));
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() || method == "POST" {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    parse_reply(&raw)
}

fn parse_reply(raw: &[u8]) -> Reply {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head");
    let head = std::str::from_utf8(&raw[..end]).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .map(|l| {
            let (n, v) = l.split_once(':').unwrap();
            (n.trim().to_owned(), v.trim().to_owned())
        })
        .collect();
    let mut body = raw[end + 4..].to_vec();
    let chunked = headers
        .iter()
        .any(|(n, v)| n.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    if chunked {
        body = dechunk(&body);
    }
    Reply {
        status,
        headers,
        body,
    }
}

fn dechunk(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").unwrap();
        let size_text = std::str::from_utf8(&data[..line_end]).unwrap();
        let size = usize::from_str_radix(size_text.split(';').next().unwrap().trim(), 16).unwrap();
        data = &data[line_end + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn get(path: &str, origin: Option<&str>, token: Option<&str>) -> Reply {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut headers = Vec::new();
    if let Some(o) = origin {
        headers.push(("Origin", o));
    }
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    http("GET", path, &headers, b"")
}

fn post(path: &str, origin: &str, token: Option<&str>, body: &serde_json::Value) -> Reply {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("Origin", origin), ("Content-Type", "application/json")];
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    http("POST", path, &headers, &serde_json::to_vec(body).unwrap())
}

fn wire(method: &str, url: &str) -> WireRequest {
    WireRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        headers: vec![],
        body: None,
        content_type: None,
        timeout_ms: Some(10_000),
        follow_redirects: true,
        max_redirects: 5,
    }
}

fn send(token: &str, request: &WireRequest) -> Reply {
    post(
        SEND,
        ORIGIN,
        Some(token),
        &serde_json::to_value(request).unwrap(),
    )
}

#[test]
fn status_needs_no_token() {
    let _agent = Agent::start();
    // Without Origin (e.g. curl): public, never paired.
    let r = get(STATUS, None, None);
    assert_eq!(r.status, 200, "{}", r.text());
    let s: Status = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(s.protocol, PROTOCOL_VERSION);
    assert_eq!(s.version, env!("CARGO_PKG_VERSION"));
    assert!(!s.paired);
    assert_eq!(r.header("access-control-allow-origin"), None);
    // From an allowed origin: readable by that origin only.
    let r = get(STATUS, Some(ORIGIN), None);
    assert_eq!(r.status, 200);
    assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(r.header("vary"), Some("Origin"));
    assert_eq!(r.json()["paired"], false);
}

#[test]
fn pairing_success_wrong_code_and_reuse() {
    let mut agent = Agent::start();
    let first = agent.code.clone();

    let wrong = post(
        PAIR,
        ORIGIN,
        None,
        &serde_json::json!({ "code": "0000-0000" }),
    );
    assert_eq!(wrong.status, 403, "{}", wrong.text());
    assert_eq!(wrong.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(wrong.json()["error"], "wrong code");

    let token = agent.pair(ORIGIN);
    assert_ne!(agent.code, first, "a new code after pairing");

    // Single use.
    let reused = post(PAIR, ORIGIN, None, &serde_json::json!({ "code": first }));
    assert_eq!(reused.status, 403);

    // The status reports the pairing for this origin and this token only.
    assert_eq!(
        get(STATUS, Some(ORIGIN), Some(&token)).json()["paired"],
        true
    );
    assert_eq!(
        get(STATUS, Some(ORIGIN), Some("nope")).json()["paired"],
        false
    );
    assert_eq!(
        get(STATUS, Some(OTHER_ORIGIN), Some(&token)).json()["paired"],
        false
    );

    // The code as typed: lower case, without the dash.
    let typed = agent.code.replace('-', "").to_lowercase();
    let r = post(
        PAIR,
        OTHER_ORIGIN,
        None,
        &serde_json::json!({ "code": typed }),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(agent.stderr().contains(&format!("paired {ORIGIN}")));

    // Bad bodies.
    let r = post(PAIR, ORIGIN, None, &serde_json::json!({ "nope": 1 }));
    assert_eq!(r.status, 400);
    let big = "x".repeat(10_000);
    let r = post(PAIR, ORIGIN, None, &serde_json::json!({ "code": big }));
    assert_eq!(r.status, 413);
}

#[test]
fn wrong_codes_are_rate_limited() {
    let mut agent = Agent::start();
    let first = agent.code.clone();
    for _ in 0..rt_cli::agent::MAX_FAILURES {
        let r = post(
            PAIR,
            ORIGIN,
            None,
            &serde_json::json!({ "code": "0000-0000" }),
        );
        assert_eq!(r.status, 403);
    }
    // The code is burned and a new one printed.
    let second = agent.next_code();
    assert_ne!(first, second);
    let r = post(PAIR, ORIGIN, None, &serde_json::json!({ "code": first }));
    assert_eq!(r.status, 403);
    agent.code = second;
    agent.pair(ORIGIN);
}

#[test]
fn send_proxies_through_the_native_transport() {
    let mut agent = Agent::start();
    let token = agent.pair(ORIGIN);
    let server = testserver();

    // Binary body, header case, query: all arrive at the target.
    let payload: Vec<u8> = (0..=255u8).collect();
    let mut request = wire("PUT", &format!("http://{server}/echo/x?secret=hunter2"));
    request.headers = vec![("X-Api-Key".into(), "hunter2".into())];
    request.body = Some(B64.encode(&payload));
    request.content_type = Some("application/octet-stream".into());
    let r = send(&token, &request);
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(r.header("access-control-expose-headers"), Some(HEAD_HEADER));
    let head = decode_head(r.header(HEAD_HEADER).expect("head header")).unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(head.url, format!("http://{server}/echo/x?secret=hunter2"));
    assert!(
        head.headers
            .iter()
            .any(|(n, v)| n == "content-type" && v.contains("json"))
    );
    let echo: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(echo["method"], "PUT");
    assert_eq!(echo["path"], "/echo/x");
    assert_eq!(echo["body_base64"], B64.encode(&payload));
    let sent_headers = echo["headers"].as_array().unwrap();
    assert!(
        sent_headers
            .iter()
            .any(|h| h[0] == "x-api-key" && h[1] == "hunter2")
    );
    assert!(
        sent_headers
            .iter()
            .any(|h| h[0] == "content-type" && h[1] == "application/octet-stream")
    );

    // A large streamed body comes back complete.
    let r = send(
        &token,
        &wire("GET", &format!("http://{server}/bytes/300000")),
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.header("transfer-encoding"), Some("chunked"), "streamed");
    assert_eq!(r.body.len(), 300_000);
    assert!(
        r.body
            .iter()
            .enumerate()
            .all(|(i, b)| usize::from(*b) == i % 251)
    );

    // Redirects are followed by the native transport and reported in the head.
    let r = send(&token, &wire("GET", &format!("http://{server}/redirect/2")));
    let head = decode_head(r.header(HEAD_HEADER).unwrap()).unwrap();
    assert_eq!(head.redirects.len(), 2);

    // The log names method and host only: no path, query, headers or body.
    let log = agent.stderr();
    assert!(log.contains(&format!("PUT {server} -> 200")), "{log}");
    assert!(!log.contains("hunter2"), "{log}");
    assert!(!log.contains("/echo"), "{log}");
}

#[test]
fn send_reports_transport_errors_and_respects_the_timeout() {
    let mut agent = Agent::start();
    let token = agent.pair(ORIGIN);
    let server = testserver();

    let mut slow = wire("GET", &format!("http://{server}/delay/3000"));
    slow.timeout_ms = Some(200);
    let started = std::time::Instant::now();
    let r = send(&token, &slow);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(r.status, 502, "{}", r.text());
    assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    let e: TransportError = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(e, TransportError::Timeout);

    // Nothing listens on port 9 on loopback.
    let r = send(&token, &wire("GET", "http://127.0.0.1:9/"));
    assert_eq!(r.status, 502);
    let e: TransportError = serde_json::from_slice(&r.body).unwrap();
    assert!(matches!(e, TransportError::Connect(_)), "{e:?}");

    let r = send(&token, &wire("GET", "not a url"));
    assert_eq!(r.status, 502);

    // Invalid requests.
    let mut bad = wire("POST", &format!("http://{server}/echo"));
    bad.body = Some("***".into());
    assert_eq!(send(&token, &bad).status, 400);
    let r = post(
        SEND,
        ORIGIN,
        Some(&token),
        &serde_json::json!({ "method": "GET" }),
    );
    assert_eq!(r.status, 400);
}

#[test]
fn send_requires_the_token_of_its_origin() {
    let mut agent = Agent::start();
    let token = agent.pair(ORIGIN);
    let request =
        serde_json::to_value(wire("GET", &format!("http://{}/echo", testserver()))).unwrap();

    for t in [None, Some("wrong"), Some("")] {
        let r = post(SEND, ORIGIN, t, &request);
        assert_eq!(r.status, 401, "{t:?}");
        // Readable by the page, so it can offer to pair again.
        assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    }
    // Another allowed origin cannot use this origin's token.
    let r = post(SEND, OTHER_ORIGIN, Some(&token), &request);
    assert_eq!(r.status, 401);
    // Not a bearer token.
    let r = http(
        "POST",
        SEND,
        &[
            ("Origin", ORIGIN),
            ("Authorization", &format!("Basic {token}")),
        ],
        &serde_json::to_vec(&request).unwrap(),
    );
    assert_eq!(r.status, 401);
    // Without Origin (not a browser page), the token alone is not enough.
    let r = http(
        "POST",
        SEND,
        &[("Authorization", &format!("Bearer {token}"))],
        &serde_json::to_vec(&request).unwrap(),
    );
    assert_eq!(r.status, 403);
    assert_eq!(post(SEND, ORIGIN, Some(&token), &request).status, 200);
    assert!(agent.stderr().contains("missing or invalid token"));
}

#[test]
fn wrong_host_is_forbidden() {
    let mut agent = Agent::start();
    let token = agent.pair(ORIGIN);
    for host in [
        "evil.example:47822",
        "evil.example",
        "127.0.0.1",
        "127.0.0.1:1234",
        "localhost.evil.example:47822",
    ] {
        let r = http("GET", STATUS, &[("Host", host), ("Origin", ORIGIN)], b"");
        assert_eq!(r.status, 403, "{host}");
        assert_eq!(r.header("access-control-allow-origin"), None, "{host}");
    }
    // Both loopback names are accepted.
    for host in ["127.0.0.1:47822", "localhost:47822", "LOCALHOST:47822"] {
        assert_eq!(
            http("GET", STATUS, &[("Host", host)], b"").status,
            200,
            "{host}"
        );
    }
    // An absolute request target carries its own host.
    let r = http("GET", "http://evil.example:47822/v1/status", &[], b"");
    assert_eq!(r.status, 403);
    // Even with a valid token.
    let r = http(
        "POST",
        SEND,
        &[
            ("Host", "evil.example:47822"),
            ("Origin", ORIGIN),
            ("Authorization", &format!("Bearer {token}")),
        ],
        b"{}",
    );
    assert_eq!(r.status, 403);
}

#[test]
fn disallowed_origins_get_no_cors_headers() {
    let mut agent = Agent::start();
    let code = agent.code.clone();
    for origin in [
        EVIL_ORIGIN,
        "null",
        "http://127.0.0.1:5173",
        "http://127.0.0.1:5187/",
    ] {
        let r = get(STATUS, Some(origin), None);
        assert_eq!(r.status, 403, "{origin}");
        assert_eq!(r.header("access-control-allow-origin"), None, "{origin}");
        let r = post(PAIR, origin, None, &serde_json::json!({ "code": code }));
        assert_eq!(r.status, 403, "{origin}");
        assert_eq!(r.header("access-control-allow-origin"), None);
        let r = http(
            "OPTIONS",
            SEND,
            &[
                ("Origin", origin),
                ("Access-Control-Request-Method", "POST"),
                ("Access-Control-Request-Private-Network", "true"),
            ],
            b"",
        );
        assert_eq!(r.status, 403, "{origin}");
        assert_eq!(r.header("access-control-allow-origin"), None);
        assert_eq!(r.header("access-control-allow-private-network"), None);
    }
    // The code was not consumed by those attempts.
    agent.pair(ORIGIN);
    assert!(agent.stderr().contains("not allowed (see --allow-origin)"));
}

#[test]
fn private_network_access_preflight() {
    let _agent = Agent::start();
    let preflight = |path: &str, method: &str, headers: &str, pna: bool| {
        let mut h = vec![
            ("Origin", ORIGIN),
            ("Access-Control-Request-Method", method),
            ("Access-Control-Request-Headers", headers),
        ];
        if pna {
            h.push(("Access-Control-Request-Private-Network", "true"));
        }
        http("OPTIONS", path, &h, b"")
    };
    let r = preflight(SEND, "POST", "authorization,content-type", true);
    assert_eq!(r.status, 204, "{}", r.text());
    assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(
        r.header("access-control-allow-private-network"),
        Some("true")
    );
    assert_eq!(r.header("access-control-allow-methods"), Some("POST"));
    let allowed = r.header("access-control-allow-headers").unwrap();
    assert!(allowed.contains("authorization") && allowed.contains("content-type"));

    let r = preflight(STATUS, "GET", "authorization", false);
    assert_eq!(r.status, 204);
    assert_eq!(r.header("access-control-allow-private-network"), None);

    // Only the methods and headers the protocol uses.
    assert_eq!(preflight(SEND, "DELETE", "", true).status, 403);
    assert_eq!(preflight(SEND, "POST", "x-other", true).status, 403);
    assert_eq!(preflight("/v1/files", "GET", "", true).status, 404);
}

#[test]
fn no_other_endpoints() {
    let mut agent = Agent::start();
    let token = agent.pair(ORIGIN);
    for path in ["/", "/v1", "/v1/files", "/v2/send", "/v1/send/x"] {
        assert_eq!(get(path, Some(ORIGIN), Some(&token)).status, 404, "{path}");
    }
    assert_eq!(
        post(STATUS, ORIGIN, None, &serde_json::json!({})).status,
        405
    );
    assert_eq!(get(SEND, Some(ORIGIN), Some(&token)).status, 405);
    // The companion does not send requests to itself.
    for url in [
        format!("http://127.0.0.1:{PORT}/v1/pair"),
        format!("http://localhost:{PORT}/v1/status"),
        format!("http://[::1]:{PORT}/"),
    ] {
        let r = send(&token, &wire("POST", &url));
        assert_eq!(r.status, 403, "{url}");
    }
}

#[test]
fn listens_on_loopback_only() {
    let agent = Agent::start();
    let mut banner = agent.seen.clone();
    while let Ok(line) = agent.lines.recv_timeout(Duration::from_secs(20)) {
        let done = line.starts_with("Stop with");
        banner.push(line);
        if done {
            break;
        }
    }
    assert!(
        banner
            .iter()
            .any(|l| l.contains(&format!("http://127.0.0.1:{PORT}")))
    );
    assert!(
        banner
            .iter()
            .any(|l| l == &format!("  {ORIGIN}/#rt-pair={}", agent.code.replace('-', ""))),
        "pairing link: {banner:?}"
    );
    // A non-loopback address of this machine, if it has one (UDP connect sends no packet).
    let local = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|()| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip());
    if let Ok(ip) = local
        && !ip.is_loopback()
        && !ip.is_unspecified()
    {
        let r = TcpStream::connect_timeout(&SocketAddr::new(ip, PORT), Duration::from_secs(2));
        assert!(r.is_err(), "rt agent answers on {ip}");
    }
}
