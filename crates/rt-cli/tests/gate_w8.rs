// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 8 gate (1) and (2), MCP part: a coding agent runs a collection over MCP; a writing
//! request against a production environment waits for approval; no secret value appears in
//! anything the agent receives or in the audit log.
//!
//! The agent is played by this test: it speaks MCP (JSON-RPC 2.0, one message per line) to
//! `rt mcp` over stdio. The API is a counting HTTP server, so the test sees exactly what
//! reached the server and when.
//!
//! The tool contract fixed by the wave 8 interface (strand C implements it):
//! - `list_requests {}` → text JSON `[{"path", "name", "method"}]`
//! - `list_environments {}` → text JSON `[{"name", "access", "production", "variables": [{"name", "secret"}]}]`
//!   (values of secret variables never appear; environments with access `none` are absent)
//! - `send_request {"path", "environment"?, "vars"?}` → the masked `SendReport` as text JSON;
//!   refused requests are `isError: true` with the reason; a writing request in a production
//!   environment waits for a decision (`rt approve <id>` / `rt deny <id>`, or the app) until
//!   `--approval-timeout` seconds, then fails with `isError: true` and sends nothing.
//! - `rt approvals --json` lists the pending approvals (`PendingApproval[]`).
//!
//! Run: `cargo test -p rt-cli --test gate_w8`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const SECRET: &str = "s3cr3t-prd-token-4711";

/// A minimal HTTP/1.1 server that answers every request with 201 `{"ok":true}` and records
/// the raw request heads.
struct CountingServer {
    port: u16,
    hits: Arc<AtomicUsize>,
    heads: Arc<Mutex<Vec<String>>>,
}

fn counting_server() -> CountingServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let heads = Arc::new(Mutex::new(Vec::new()));
    let (h, r) = (hits.clone(), heads.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            r.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).into_owned());
            h.fetch_add(1, Ordering::SeqCst);
            let body = r#"{"ok":true}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    CountingServer { port, hits, heads }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n\nextensions:\n  requesttailor:\n    agents:\n      access: read\n      environments:\n        Local: { access: write }\n        PRD: { access: write, production: true }\n        Vault: { access: none }\n",
    );
    for env in ["Local", "PRD", "Vault"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
            ),
        );
    }
    write(
        &root.join("orders/list.yml"),
        "info:\n  name: List orders\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/orders?token={{token}}\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n",
    );
    write(
        &root.join("orders/create.yml"),
        "info:\n  name: Create order\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{base}}/orders\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n  body:\n    type: json\n    data: '{\"item\": \"tea\", \"auth\": \"{{token}}\"}'\n",
    );
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// Every line the agent received, for the secret check.
    seen: Vec<String>,
    next_id: u64,
}

impl Mcp {
    fn start(root: &Path, data: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .args(["mcp", root.to_str().unwrap()])
            .args(["--var", &format!("token={SECRET}")])
            .args(["--approval-timeout", "20"])
            .env("RT_DATA_DIR", data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut mcp = Mcp {
            child,
            stdin,
            stdout,
            seen: Vec::new(),
            next_id: 1,
        };
        let init = mcp.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "gate-agent", "version": "1"}
            }),
        );
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        mcp.notify("notifications/initialized", json!({}));
        mcp
    }

    fn notify(&mut self, method: &str, params: Value) {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
    }

    /// Sends a request and reads lines until its response (notifications are recorded too).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "rt mcp closed stdout"
            );
            self.seen.push(line.clone());
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))["result"].clone()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn rt(args: &[&str], data: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(args)
        .env("RT_DATA_DIR", data)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-gate-w8-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn agent_runs_collection_over_mcp_and_production_writes_wait_for_approval() {
    let server = counting_server();
    let dir = tmp("approval");
    let (root, data) = (dir.join("collection"), dir.join("data"));
    collection(&root, server.port);
    let mut mcp = Mcp::start(&root, &data);

    let tools = mcp.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for want in [
        "list_requests",
        "get_request",
        "list_environments",
        "send_request",
        "run_folder",
    ] {
        assert!(names.contains(&want), "missing tool {want} in {names:?}");
    }

    let requests = text(&mcp.call("list_requests", json!({})));
    assert!(requests.contains("orders/create.yml"), "{requests}");
    let envs = text(&mcp.call("list_environments", json!({})));
    assert!(envs.contains("PRD") && !envs.contains("Vault"), "{envs}");

    // Reading against production: allowed, sent at once.
    let list = mcp.call(
        "send_request",
        json!({"path": "orders/list.yml", "environment": "PRD"}),
    );
    assert_ne!(list["isError"], json!(true), "{list}");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);

    // Writing against a non-production environment with write access: sent at once.
    let local = mcp.call(
        "send_request",
        json!({"path": "orders/create.yml", "environment": "Local"}),
    );
    assert_ne!(local["isError"], json!(true), "{local}");
    assert_eq!(server.hits.load(Ordering::SeqCst), 2);

    // Writing against production: waits. A second thread plays the user and approves.
    let approver_data = data.clone();
    let approver = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let (_, out) = rt(&["approvals", "--json"], &approver_data);
            let pending: Value = serde_json::from_str(&out).unwrap_or(json!([]));
            if let Some(first) = pending.as_array().and_then(|a| a.first()) {
                assert_eq!(first["request_path"], "orders/create.yml");
                assert_eq!(first["environment"], "PRD");
                assert!(!out.contains(SECRET), "secret in the approval: {out}");
                return first["id"].as_str().unwrap().to_owned();
            }
            assert!(Instant::now() < deadline, "no pending approval appeared");
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let hits_before = server.hits.load(Ordering::SeqCst);
    let id_thread = std::thread::spawn({
        let data = data.clone();
        move || {
            let id = approver.join().unwrap();
            let (code, _) = rt(&["approve", &id], &data);
            assert_eq!(code, 0, "rt approve failed");
        }
    });
    let prd = mcp.call(
        "send_request",
        json!({"path": "orders/create.yml", "environment": "PRD"}),
    );
    id_thread.join().unwrap();
    assert_ne!(prd["isError"], json!(true), "{prd}");
    assert_eq!(server.hits.load(Ordering::SeqCst), hits_before + 1);

    // The server got the real token; the agent never saw it.
    assert!(
        server
            .heads
            .lock()
            .unwrap()
            .iter()
            .all(|h| h.contains(SECRET))
    );
    for line in &mcp.seen {
        assert!(
            !line.contains(SECRET),
            "secret value in the agent context: {line}"
        );
    }
    let audit = std::fs::read_to_string(data.join("agents/audit.jsonl")).unwrap();
    assert!(!audit.contains(SECRET), "secret value in the audit log");
    let entries: Vec<Value> = audit
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(entries.len() >= 6, "one audit entry per tool call: {audit}");
    assert!(
        entries
            .iter()
            .any(|e| e["outcome"] == "sent" && e["approved_by"].is_string()),
        "{audit}"
    );
}

#[test]
fn production_write_without_a_decision_times_out_and_sends_nothing() {
    let server = counting_server();
    let dir = tmp("timeout");
    let (root, data) = (dir.join("collection"), dir.join("data"));
    collection(&root, server.port);
    let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["mcp", root.to_str().unwrap(), "--approval-timeout", "1"])
        .args(["--var", &format!("token={SECRET}")])
        .env("RT_DATA_DIR", &data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let send = |stdin: &mut ChildStdin, v: Value| writeln!(stdin, "{v}").unwrap();
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "gate-agent", "version": "1"}}}),
    );
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}),
    );
    send(
        &mut stdin,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "send_request", "arguments": {"path": "orders/create.yml", "environment": "PRD"}}}),
    );
    let mut result = Value::Null;
    for line in stdout.by_ref().lines() {
        let line = line.unwrap();
        assert!(!line.contains(SECRET));
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == json!(2) {
            result = v["result"].clone();
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(result["isError"], json!(true), "{result}");
    assert!(text(&result).to_lowercase().contains("approv"), "{result}");
    assert_eq!(
        server.hits.load(Ordering::SeqCst),
        0,
        "sent without approval"
    );
    let (_, pending) = rt(&["approvals", "--json"], &data);
    assert_eq!(pending.trim(), "[]", "the timed-out approval must be gone");
}

#[test]
fn read_only_environment_refuses_writes() {
    let server = counting_server();
    let dir = tmp("readonly");
    let (root, data) = (dir.join("collection"), dir.join("data"));
    collection(&root, server.port);
    // Default access is `read`; an environment not listed falls back to it.
    write(
        &root.join("environments/Staging.yml"),
        &format!(
            "name: Staging\nvariables:\n  - name: base\n    value: http://127.0.0.1:{}\n",
            server.port
        ),
    );
    let mut mcp = Mcp::start(&root, &data);
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/create.yml", "environment": "Staging"}),
    );
    assert_eq!(r["isError"], json!(true), "{r}");
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/list.yml", "environment": "Vault"}),
    );
    assert_eq!(r["isError"], json!(true), "access none: {r}");
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
}
