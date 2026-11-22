// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! External secrets in `rt send` and through the companion (`rt agent --allow-secrets`,
//! `POST /v1/secrets`), wave 5, strand E. A fake `op` (a shell script first on `PATH`) stands
//! in for the 1Password CLI, so these tests run on Unix only.
//!
//! The agent tests share one port (47826), so they run one at a time (`AGENT_LOCK`).

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use rt_core::model::{ExternalSecret, SecretProvider};
use rt_core::transport::companion::{PairResponse, SecretsError, SecretsRequest, SecretsResponse};

const PORT: u16 = 47826;
const ORIGIN: &str = "http://127.0.0.1:5191";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("rt-ext-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A secret value, assembled at run time.
fn value() -> String {
    ["s3cret", "-from-", "op-7f3a"].concat()
}

/// A folder with a fake `op`: `op read --no-newline op://Dev/api/token` prints the value,
/// `op://Dev/api/locked` fails as when signed out; it logs its arguments to `op.log`.
fn fake_op(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> '{log}'\ncase \"$3\" in\n  op://Dev/api/token) printf '%s' '{value}' ;;\n  *) echo '[ERROR] You are not currently signed in.' >&2; exit 1 ;;\nesac\n",
        log = dir.join("op.log").display(),
        value = value()
    );
    write(&bin.join("op"), &script);
    std::fs::set_permissions(bin.join("op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn path_with(bin: &Path) -> String {
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

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

fn collection(dir: &Path, reference: &str) {
    write(
        &dir.join("col/opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: ext\n",
    );
    write(
        &dir.join("col/environments/Dev.yml"),
        &format!(
            "name: Dev\nvariables:\n  - name: host\n    value: http://{}\nexternalSecrets:\n  type: one-password\n  variables:\n    - name: token\n      secretReference: {reference}\n",
            testserver()
        ),
    );
    write(
        &dir.join("col/me.yml"),
        &format!(
            "info:\n  name: me\n  type: http\n\nhttp:\n  method: GET\n  url: \"{{{{host}}}}/bearer/{}\"\n  auth:\n    type: bearer\n    token: \"{{{{token}}}}\"\n",
            value()
        ),
    );
}

fn rt_send(dir: &Path, bin: &Path, extra: &[&str]) -> (i32, serde_json::Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["send", "col/me.yml", "--env", "Dev"])
        .args(extra)
        .current_dir(dir)
        .env("PATH", path_with(bin))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let report = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    (
        out.status.code().unwrap_or(-1),
        report,
        format!("{stdout}{}", String::from_utf8_lossy(&out.stderr)),
    )
}

#[test]
fn rt_send_reads_the_secret_from_the_cli_and_masks_it() {
    let d = temp("send");
    let bin = fake_op(&d.0);
    collection(&d.0, "op://Dev/api/token");
    let (code, report, all) = rt_send(&d.0, &bin, &[]);
    assert_eq!(code, 0, "{all}");
    // rt-testserver answers 200 only when the bearer token is the one in the path.
    assert_eq!(report["response"]["status"], 200, "{all}");
    // The sent request is masked. (The response is reported as received: its URL carries the
    // value here only because this test server takes the expected token in the path.)
    let request = report["request"].to_string();
    assert!(!request.contains(&value()), "the value leaked: {request}");
    assert_eq!(
        report["request"]["url"],
        format!("http://{}/bearer/***", testserver())
    );
    let variables = report["variables"].to_string();
    assert!(!variables.contains(&value()), "{variables}");
    let headers = report["request"]["headers"].to_string();
    assert!(headers.contains("Bearer ***"), "{headers}");
    let log = std::fs::read_to_string(d.0.join("op.log")).unwrap();
    assert_eq!(log, "read --no-newline op://Dev/api/token\n");
}

#[test]
fn rt_send_explains_a_missing_login_and_var_skips_the_cli() {
    let d = temp("locked");
    let bin = fake_op(&d.0);
    collection(&d.0, "op://Dev/api/locked");
    let (code, report, all) = rt_send(&d.0, &bin, &[]);
    assert_eq!(code, 3, "{all}");
    let message = report["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("1Password: not signed in"), "{all}");
    assert!(message.contains("op signin"), "{all}");
    // A pipeline passes the value itself: the CLI is not run.
    std::fs::remove_file(d.0.join("op.log")).unwrap();
    let token = format!("--var=token={}", value());
    let (code, report, all) = rt_send(&d.0, &bin, &[&token]);
    assert_eq!(code, 0, "{all}");
    assert_eq!(report["response"]["status"], 200, "{all}");
    assert!(!d.0.join("op.log").exists());
}

#[test]
fn rt_send_without_the_cli_says_how_to_install_it() {
    let d = temp("nocli");
    collection(&d.0, "op://Dev/api/token");
    let empty = d.0.join("empty-bin");
    std::fs::create_dir_all(&empty).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["send", "col/me.yml", "--env", "Dev"])
        .current_dir(&d.0)
        .env("PATH", &empty)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(3));
    let message = report["error"]["message"].as_str().unwrap();
    assert!(message.contains("`op` was not found"), "{message}");
    assert!(message.contains("1Password CLI"), "{message}");
}

// ---------------------------------------------------------------------------------------------
// The companion's `POST /v1/secrets`

static AGENT_LOCK: Mutex<()> = Mutex::new(());

struct Agent {
    child: Child,
    lines: Receiver<String>,
    stderr: Arc<Mutex<String>>,
    _dir: Dir,
    _lock: MutexGuard<'static, ()>,
}

impl Agent {
    fn start(name: &str, allow_secrets: bool) -> (Self, String) {
        let lock = AGENT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp(name);
        let bin = fake_op(&dir.0);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rt"));
        cmd.args([
            "agent",
            "--port",
            &PORT.to_string(),
            "--allow-origin",
            ORIGIN,
        ]);
        if allow_secrets {
            cmd.arg("--allow-secrets");
        }
        let mut child = cmd
            .env("PATH", path_with(&bin))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
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
        let agent = Agent {
            child,
            lines,
            stderr,
            _dir: dir,
            _lock: lock,
        };
        let mut banner = Vec::new();
        let code = loop {
            let line = agent
                .lines
                .recv_timeout(Duration::from_secs(20))
                .expect("pairing code");
            if let Some(rest) = line.strip_prefix("Pairing code: ") {
                break rest.split_whitespace().next().unwrap().to_owned();
            }
            banner.push(line);
        };
        assert_eq!(
            banner.iter().any(|l| l.starts_with("External secrets: on")),
            allow_secrets,
            "{banner:?}"
        );
        (agent, code)
    }

    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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
}

/// A raw HTTP/1.1 exchange with the agent (connection closed after one request).
fn http(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
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
    let mut stream = stream.expect("agent listening");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (n, v) in headers {
        req.push_str(&format!("{n}: {v}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .collect();
    Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

fn pair(code: &str) -> String {
    let body = serde_json::json!({ "code": code }).to_string();
    let r = http(
        "POST",
        "/v1/pair",
        &[("Origin", ORIGIN), ("Content-Type", "application/json")],
        body.as_bytes(),
    );
    assert_eq!(r.status, 200);
    serde_json::from_slice::<PairResponse>(&r.body)
        .unwrap()
        .token
}

fn secrets_request(reference: &str) -> Vec<u8> {
    serde_json::to_vec(&SecretsRequest {
        provider: SecretProvider::OnePassword,
        variables: vec![
            ExternalSecret {
                name: "token".into(),
                reference: reference.into(),
                enabled: true,
            },
            ExternalSecret {
                name: "off".into(),
                reference: "op://Dev/api/off".into(),
                enabled: false,
            },
        ],
    })
    .unwrap()
}

fn post_secrets(token: Option<&str>, origin: Option<&str>, body: &[u8]) -> Reply {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(a) = &auth {
        headers.push(("Authorization", a));
    }
    if let Some(o) = origin {
        headers.push(("Origin", o));
    }
    http("POST", "/v1/secrets", &headers, body)
}

#[test]
fn the_companion_reads_secrets_for_a_paired_origin() {
    let (agent, code) = Agent::start("agent-ok", true);
    let token = pair(&code);
    let r = post_secrets(
        Some(&token),
        Some(ORIGIN),
        &secrets_request("op://Dev/api/token"),
    );
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let answer: SecretsResponse = serde_json::from_slice(&r.body).unwrap();
    // Only the enabled entry.
    assert_eq!(answer.values, [("token".to_owned(), value())]);
    // The log names the provider and the count, never the value or the reference.
    let log = agent.stderr();
    assert!(
        log.contains("read 1 secret(s) from 1Password for http://127.0.0.1:5191"),
        "{log}"
    );
    assert!(!log.contains(&value()), "{log}");
    assert!(!log.contains("op://"), "{log}");

    // A secret that cannot be read: 422 with the reason.
    let r = post_secrets(
        Some(&token),
        Some(ORIGIN),
        &secrets_request("op://Dev/api/locked"),
    );
    assert_eq!(r.status, 422);
    let e: SecretsError = serde_json::from_slice(&r.body).unwrap();
    assert!(e.error.contains("1Password: not signed in"), "{}", e.error);
    // Option injection is refused before anything runs.
    let r = post_secrets(
        Some(&token),
        Some(ORIGIN),
        &secrets_request("op://x --account evil"),
    );
    assert_eq!(r.status, 422);

    // The preflight allows the endpoint for the allowed origin.
    let r = http(
        "OPTIONS",
        "/v1/secrets",
        &[
            ("Origin", ORIGIN),
            ("Access-Control-Request-Method", "POST"),
            (
                "Access-Control-Request-Headers",
                "authorization, content-type",
            ),
        ],
        b"",
    );
    assert_eq!(r.status, 204);
    assert_eq!(r.header("access-control-allow-methods"), Some("POST"));
}

#[test]
fn the_secrets_endpoint_needs_token_origin_host_and_the_flag() {
    let (agent, code) = Agent::start("agent-checks", true);
    let token = pair(&code);
    let body = secrets_request("op://Dev/api/token");
    // No token, a wrong token: 401.
    assert_eq!(post_secrets(None, Some(ORIGIN), &body).status, 401);
    assert_eq!(post_secrets(Some("nope"), Some(ORIGIN), &body).status, 401);
    // Another origin, even with a valid token: 403 without CORS headers.
    let r = post_secrets(Some(&token), Some("https://evil.example"), &body);
    assert_eq!(r.status, 403);
    assert_eq!(r.header("access-control-allow-origin"), None);
    // No origin (not a browser page): 403.
    assert_eq!(post_secrets(Some(&token), None, &body).status, 403);
    // A rebound host name: 403.
    let auth = format!("Bearer {token}");
    let r = {
        let mut s = TcpStream::connect(("127.0.0.1", PORT)).unwrap();
        let req = format!(
            "POST /v1/secrets HTTP/1.1\r\nHost: evil.example:{PORT}\r\nOrigin: {ORIGIN}\r\nAuthorization: {auth}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        s.write_all(&body).unwrap();
        let mut raw = String::new();
        let _ = s.read_to_string(&mut raw);
        raw
    };
    assert!(r.starts_with("HTTP/1.1 403"), "{r}");
    assert!(!agent.stderr().contains(&value()));
    drop(agent);

    // Without --allow-secrets: 403 with how to turn it on, and nothing runs.
    let (agent, code) = Agent::start("agent-off", false);
    let token = pair(&code);
    let r = post_secrets(Some(&token), Some(ORIGIN), &body);
    assert_eq!(r.status, 403);
    assert_eq!(r.header("access-control-allow-origin"), Some(ORIGIN));
    let e: SecretsError = serde_json::from_slice(&r.body).unwrap();
    assert!(e.error.contains("rt agent --allow-secrets"), "{}", e.error);
    assert!(!agent._dir.0.join("op.log").exists());
}
