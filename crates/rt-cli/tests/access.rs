// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand D: `rt access check`. A tiny owner-aware server answers the verdicts
//! `ok` and `inconclusive`; `testdata/insight/orders-api.mjs` (a real service with the two
//! deliberate bugs, on port 48716) answers `broken-object-access` and
//! `broken-function-access`. Exit 4 for broken access, never a secret or a personal value in
//! the output, writing requests only with `--allow-writes` and never against production.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rt_app::access::AccessVerdict;

const SECRET: &str = "RT-CANARY-access-cli-3b9e";
const PERSONAL: &str = "carol.canary@example.test";
/// The port of the orders API of this strand (`docs`: strand D, 48715 to 48719).
const ORDERS_PORT: u16 = 48716;

struct Server {
    port: u16,
    hits: Arc<AtomicUsize>,
}

/// `GET /orders/1` belongs to alice; somebody else gets `other` (403, 404, 500, 200).
fn owner_server(other: u16) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            counter.fetch_add(1, Ordering::SeqCst);
            let mine = head.contains("x-user: alice");
            let (status, body) = if head.starts_with("get /orders/1") && (mine || other == 200) {
                (
                    200,
                    format!("{{\"id\":1,\"owner\":\"alice\",\"email\":\"{PERSONAL}\"}}"),
                )
            } else if head.starts_with("get /orders/1") {
                (other, "{\"error\":\"no\"}".to_owned())
            } else if head.starts_with("get /health") {
                (200, "{\"ok\":true}".to_owned())
            } else {
                (404, "{\"error\":\"not found\"}".to_owned())
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Server { port, hits }
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-access-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n",
    );
    for (env, user) in [
        ("alice", "alice"),
        ("bob", "bob"),
        ("PRD", "bob"),
        ("admin", "admin"),
    ] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - name: user\n    value: {user}\n  - name: orderId\n    value: \"1\"\n  - secret: true\n    name: token\n"
            ),
        );
    }
    let headers = "  headers:\n    - name: x-user\n      value: \"{{user}}\"\n    - name: Authorization\n      value: Bearer {{token}}\n";
    write(
        &root.join("get.yml"),
        &format!(
            "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/orders/{{{{orderId}}}}\"\n{headers}"
        ),
    );
    write(
        &root.join("health.yml"),
        &format!(
            "info:\n  name: Health\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/health\"\n{headers}"
        ),
    );
    write(
        &root.join("put.yml"),
        &format!(
            "info:\n  name: Put\n  type: http\n  seq: 3\n\nhttp:\n  method: PUT\n  url: \"{{{{base}}}}/orders/{{{{orderId}}}}\"\n{headers}"
        ),
    );
    write(
        &root.join("clear.yml"),
        &format!(
            "info:\n  name: Clear\n  type: http\n  seq: 4\n\nhttp:\n  method: DELETE\n  url: \"{{{{base}}}}/admin/orders\"\n{headers}"
        ),
    );
}

fn rt(dir: &Path, file: &Path, profiles: [&str; 2], extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["access", "check"])
        .arg(file)
        .args(["--as", profiles[0], "--as", profiles[1]])
        .args([
            "--var",
            &format!("token={SECRET}"),
            "--no-plugins",
            "--rate",
            "1000",
        ])
        .args(extra)
        .env("RT_DATA_DIR", dir.join("data"))
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn audit(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("data/agents/audit.jsonl")).unwrap_or_default()
}

fn clean(o: &Output, dir: &Path) {
    for text in [stdout(o), stderr(o), audit(dir)] {
        assert!(!text.contains(SECRET) && !text.contains(PERSONAL), "{text}");
    }
}

#[test]
fn a_denied_b_is_ok_and_exits_zero() {
    for status in [403, 404] {
        let srv = owner_server(status);
        let dir = tmp("ok");
        collection(&dir.join("c"), srv.port);
        let out = rt(&dir, &dir.join("c/get.yml"), ["alice", "bob"], &[]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}{}",
            stdout(&out),
            stderr(&out)
        );
        assert!(stdout(&out).starts_with("ok  get.yml"), "{}", stdout(&out));
        assert_eq!(srv.hits.load(Ordering::SeqCst), 2);
        clean(&out, &dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_server_error_for_b_is_inconclusive_and_exits_zero() {
    let srv = owner_server(500);
    let dir = tmp("inconclusive");
    collection(&dir.join("c"), srv.port);
    let out = rt(&dir, &dir.join("c/get.yml"), ["alice", "bob"], &["--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["verdict"]["kind"], "inconclusive");
    assert_eq!(v["status_b"], 500);
    clean(&out, &dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn leaking_object_access_exits_4_and_evidence_holds_no_values() {
    let srv = owner_server(200);
    let dir = tmp("leak");
    collection(&dir.join("c"), srv.port);
    let out = rt(&dir, &dir.join("c/get.yml"), ["alice", "bob"], &[]);
    assert_eq!(
        out.status.code(),
        Some(4),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let text = stdout(&out);
    assert!(text.starts_with("BROKEN OBJECT ACCESS  get.yml"), "{text}");
    assert!(text.contains("B received object 1"), "{text}");
    clean(&out, &dir);
    // The audit log has the run.
    assert!(audit(&dir).contains("access-check"), "{}", audit(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_an_object_reference_nothing_is_guessed() {
    let srv = owner_server(200);
    let dir = tmp("noref");
    collection(&dir.join("c"), srv.port);
    let out = rt(&dir, &dir.join("c/health.yml"), ["alice", "bob"], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("no object reference found"),
        "{}",
        stderr(&out)
    );
    assert_eq!(srv.hits.load(Ordering::SeqCst), 1, "B was not sent");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writing_requests_need_allow_writes_and_never_run_against_production() {
    let srv = owner_server(200);
    let dir = tmp("writes");
    collection(&dir.join("c"), srv.port);
    let file = dir.join("c/put.yml");
    let out = rt(&dir, &file, ["alice", "bob"], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("--allow-writes"), "{}", stderr(&out));
    assert_eq!(srv.hits.load(Ordering::SeqCst), 0);

    let out = rt(&dir, &file, ["alice", "PRD"], &["--allow-writes"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("skipped (production)"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        srv.hits.load(Ordering::SeqCst),
        0,
        "nothing went to production"
    );
    clean(&out, &dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn arguments_are_checked() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let run = |args: &[&str], out: &mut Vec<u8>, err: &mut Vec<u8>| {
        rt_cli::run(args.iter().copied().map(String::from), out, err)
    };
    assert_eq!(
        run(
            &["access", "check", "a.yml", "--as", "A"],
            &mut out,
            &mut err
        ),
        2
    );
    assert!(String::from_utf8_lossy(&err).contains("exactly two"));
    assert_eq!(
        run(
            &[
                "access",
                "check",
                "/nonexistent/a.yml",
                "--as",
                "A",
                "--as",
                "B"
            ],
            &mut out,
            &mut err
        ),
        2
    );
    assert_eq!(
        rt_cli::access::exit_code(&AccessVerdict::BrokenObjectAccess),
        4
    );
    assert_eq!(
        rt_cli::access::exit_code(&AccessVerdict::BrokenFunctionAccess),
        4
    );
    assert_eq!(rt_cli::access::exit_code(&AccessVerdict::Ok), 0);
    assert_eq!(rt_cli::access::exit_code(&AccessVerdict::Inconclusive), 0);
    assert_eq!(
        rt_cli::access::exit_code(&AccessVerdict::Skipped {
            reason: "refused by the send guard (r): x".into()
        }),
        rt_cli::EXIT_BLOCKED
    );
}

// ---------------------------------------------------------------------------------------------
// The real orders API of the fixtures

struct Node(Child);

impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn orders_api() -> Node {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/insight/orders-api.mjs");
    let child = Command::new("node")
        .arg(script)
        .arg(ORDERS_PORT.to_string())
        .stderr(Stdio::null())
        .spawn()
        .expect("node runs testdata/insight/orders-api.mjs");
    let node = Node(child);
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", ORDERS_PORT)).is_ok() {
            return node;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("orders-api did not start on {ORDERS_PORT}");
}

/// A raw request to the orders API (alice creates order 1).
fn call(method: &str, path: &str, user: &str, body: &str) {
    let mut stream = TcpStream::connect(("127.0.0.1", ORDERS_PORT)).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: x\r\nx-user: {user}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut sink = String::new();
    let _ = stream.read_to_string(&mut sink);
}

#[test]
fn the_orders_api_leaks_objects_and_functions() {
    let _node = orders_api();
    call(
        "POST",
        "/orders",
        "alice",
        &format!("{{\"item\":\"book\",\"email\":\"{PERSONAL}\"}}"),
    );
    let dir = tmp("orders");
    collection(&dir.join("c"), ORDERS_PORT);

    // Object level: bob reads alice's order.
    let out = rt(&dir, &dir.join("c/get.yml"), ["alice", "bob"], &[]);
    assert_eq!(
        out.status.code(),
        Some(4),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("B received object 1"),
        "{}",
        stdout(&out)
    );
    clean(&out, &dir);

    // Function level: `DELETE /admin/orders` needs `admin`, but bob may call it. A write: it
    // needs the permission, and runs for non-production environments only.
    let clear = dir.join("c/clear.yml");
    let out = rt(&dir, &clear, ["admin", "bob"], &["--b-must-be-denied"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let out = rt(
        &dir,
        &clear,
        ["admin", "bob"],
        &["--b-must-be-denied", "--allow-writes", "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(4),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["verdict"]["kind"], "broken-function-access");
    clean(&out, &dir);
    // Both runs are in the audit log, the writing one with its approval.
    let log = audit(&dir);
    assert!(log.matches("access-check").count() >= 3, "{log}");
    assert!(log.contains("\"approved_by\":\"cli\""), "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}
