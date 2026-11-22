// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The built `rt` binary: `rt mock` serves the fixture collection over HTTP (port 0, so it
//! never clashes), `rt docs` writes the documentation (wave 8, strand F).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn shop() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/shop")
}

/// `rt mock` on a free port; killed when dropped.
struct MockServer {
    child: Child,
    addr: String,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(args: &[&str]) -> MockServer {
    let data = std::env::temp_dir().join(format!("rt-mock-test-data-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("mock")
        .args(args)
        .env("RT_DATA_DIR", data)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start rt mock");
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    let addr = first
        .trim()
        .strip_prefix("rt mock: listening on http://")
        .unwrap_or_else(|| panic!("unexpected banner {first:?}"))
        .to_owned();
    MockServer { child, addr }
}

struct Reply {
    status: u16,
    head: String,
    body: String,
}

/// One HTTP/1.1 request with `Connection: close`.
fn call(addr: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (n, v) in headers {
        req.push_str(&format!("{n}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    Reply {
        status,
        head: head.to_ascii_lowercase(),
        body: body.to_owned(),
    }
}

#[test]
fn mock_serves_saved_examples_over_http() {
    let shop = shop();
    let server = start(&[shop.to_str().unwrap(), "--port", "0", "--env", "Local"]);
    assert!(server.addr.starts_with("127.0.0.1:"), "{}", server.addr);

    let found = call(&server.addr, "GET", "/api/v1/users/42", &[], "");
    assert_eq!(found.status, 200);
    assert!(found.head.starts_with("http/1.1 200 ok"), "{}", found.head);
    assert!(
        found.head.contains("x-request-id: example-1"),
        "{}",
        found.head
    );
    assert!(
        found.head.contains("access-control-allow-origin: *"),
        "{}",
        found.head
    );
    assert!(found.body.contains("\"Ada\""), "{}", found.body);

    let created = call(
        &server.addr,
        "POST",
        "/api/v1/users",
        &[("Content-Type", "application/json")],
        "{\"name\":\"Grace\"}",
    );
    assert_eq!(created.status, 201);
    assert!(
        created.head.contains("location: /api/v1/users/43"),
        "{}",
        created.head
    );

    let preflight = call(
        &server.addr,
        "OPTIONS",
        "/api/v1/users",
        &[
            ("Origin", "http://127.0.0.1:5211"),
            ("Access-Control-Request-Method", "POST"),
            (
                "Access-Control-Request-Headers",
                "authorization, content-type",
            ),
        ],
        "",
    );
    assert_eq!(preflight.status, 204);
    assert!(
        preflight
            .head
            .contains("access-control-allow-headers: authorization, content-type")
    );

    let none = call(&server.addr, "GET", "/nothing", &[], "");
    assert_eq!(none.status, 404);
    let json: serde_json::Value = serde_json::from_str(&none.body).unwrap();
    assert_eq!(json["routes"].as_array().unwrap().len(), 4);
}

#[test]
fn mock_refuses_what_it_cannot_serve() {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["mock", "/nonexistent/collection"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("rt mock: cannot open"));

    let help = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["mock", "--help"])
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("rt mock:"));
}

#[test]
fn docs_writes_a_static_page() {
    let out = std::env::temp_dir().join(format!("rt-docs-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let shop = shop();
    let result = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args([
            "docs",
            shop.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("Get user"));
    assert!(!html.contains("<script"));
    assert!(out.join("style.css").is_file());
    let _ = std::fs::remove_dir_all(&out);

    let missing = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["docs", shop.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
}
