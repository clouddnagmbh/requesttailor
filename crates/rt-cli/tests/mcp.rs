// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt mcp` beyond the gate (wave 8, strand C): the protocol, secrets at the boundary, one
//! audit entry per call, denial, elicitation, `run_folder`, script trust, the rules for agent
//! variables and the pinned policy.
//!
//! The API is an echo server: every response body is the raw request head, so a secret in
//! a header comes straight back in the body; only the boundary masking keeps it from the
//! agent.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const SECRET: &str = "very-secret-token-0815";
/// The value the fake secret manager (`op`) returns.
const EXTERNAL: &str = "op-secret-value-4242";

struct Echo {
    port: u16,
    hits: Arc<AtomicUsize>,
    methods: Arc<Mutex<Vec<String>>>,
}

fn echo_server() -> Echo {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let methods = Arc::new(Mutex::new(Vec::new()));
    let (h, m) = (hits.clone(), methods.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            m.lock()
                .unwrap()
                .push(head.split(' ').next().unwrap_or_default().to_owned());
            h.fetch_add(1, Ordering::SeqCst);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{head}",
                head.len()
            );
        }
    });
    Echo {
        port,
        hits,
        methods,
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn path_with(bin: &Path) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    format!(
        "{}{sep}{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

/// A fake 1Password CLI in `data/bin`: `op read --no-newline op://Dev/api/key` prints
/// [`EXTERNAL`].
#[cfg(unix)]
fn fake_op(data: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let op = data.join("bin/op");
    write(
        &op,
        &format!(
            "#!/bin/sh\ncase \"$3\" in\n  op://Dev/api/key) printf '%s' '{EXTERNAL}' ;;\n  *) exit 1 ;;\nesac\n"
        ),
    );
    std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const POLICY: &str = "extensions:\n  requesttailor:\n    agents:\n      access: read\n      environments:\n        Local: { access: write }\n        PRD: { access: write, production: true }\n";

/// Orders collection: `orders/list.yml` (GET), `orders/create.yml` (POST), an environment
/// per name with a `base`, the secret `token` (from `--var`) and the secret `apikey` (read
/// from a secret manager in the echo test).
fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        &format!("opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n\n{POLICY}"),
    );
    for env in ["Local", "PRD", "Staging"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n  - secret: true\n    name: apikey\n"
            ),
        );
    }
    write(
        &root.join("orders/folder.yml"),
        "info:\n  name: orders\n  type: folder\n  seq: 1\n",
    );
    write(
        &root.join("orders/list.yml"),
        "info:\n  name: List orders\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/orders?page={{page}}\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n    - name: X-Api-Key\n      value: \"{{apikey}}\"\n\nruntime:\n  variables:\n    - name: page\n      value: \"1\"\n",
    );
    write(
        &root.join("orders/create.yml"),
        "info:\n  name: Create order\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{base}}/orders\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n  body:\n    type: json\n    data: '{\"item\": \"tea\"}'\n",
    );
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: Arc<Mutex<String>>,
    seen: Vec<String>,
    next_id: u64,
}

impl Mcp {
    fn spawn(root: &Path, data: &Path, extra: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .args(["mcp", root.to_str().unwrap()])
            .args(["--var", &format!("token={SECRET}")])
            .args(["--approval-timeout", "15"])
            .args(extra)
            .env("RT_DATA_DIR", data)
            // A fake secret manager CLI, if a test put one there.
            .env("PATH", path_with(&data.join("bin")))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = Arc::new(Mutex::new(String::new()));
        let pipe = BufReader::new(child.stderr.take().unwrap());
        let sink = stderr.clone();
        std::thread::spawn(move || {
            for line in pipe.lines().map_while(Result::ok) {
                let mut s = sink.lock().unwrap();
                s.push_str(&line);
                s.push('\n');
            }
        });
        Mcp {
            child,
            stdin,
            stdout,
            stderr,
            seen: Vec::new(),
            next_id: 1,
        }
    }

    fn start(root: &Path, data: &Path, capabilities: Value, extra: &[&str]) -> Self {
        let mut mcp = Self::spawn(root, data, extra);
        let init = mcp.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": capabilities,
                "clientInfo": {"name": "test-agent", "version": "1"}
            }),
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
        mcp.notify("notifications/initialized", json!({}));
        mcp
    }

    fn raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.raw(&json!({"jsonrpc": "2.0", "method": method, "params": params}).to_string());
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        assert!(
            self.stdout.read_line(&mut line).unwrap() > 0,
            "rt mcp closed stdout"
        );
        self.seen.push(line.clone());
        serde_json::from_str(&line).unwrap()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.raw(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
        );
        loop {
            let v = self.read();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))["result"].clone()
    }

    fn assert_no_secrets(&self) {
        for line in &self.seen {
            assert!(
                !line.contains(SECRET),
                "secret in the agent context: {line}"
            );
            assert!(
                !line.contains(EXTERNAL),
                "stored secret in the agent context: {line}"
            );
        }
        let err = self.stderr.lock().unwrap();
        assert!(!err.contains(SECRET), "secret on stderr: {err}");
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

fn json_text(result: &Value) -> Value {
    serde_json::from_str(&text(result)).unwrap_or(Value::Null)
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

fn audit(data: &Path) -> Vec<Value> {
    std::fs::read_to_string(data.join("agents/audit.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Plays the user: waits for the first pending approval and answers with `command`.
fn answer_first(data: &Path, command: &'static str) -> std::thread::JoinHandle<Value> {
    let data = data.to_path_buf();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let (_, out) = rt(&["approvals", "--json"], &data);
            let pending: Value = serde_json::from_str(&out).unwrap_or(json!([]));
            if let Some(first) = pending.as_array().and_then(|a| a.first()).cloned() {
                let (code, _) = rt(&[command, first["id"].as_str().unwrap()], &data);
                assert_eq!(code, 0, "rt {command}");
                return first;
            }
            assert!(Instant::now() < deadline, "no pending approval appeared");
            std::thread::sleep(Duration::from_millis(50));
        }
    })
}

fn setup(name: &str) -> (Echo, PathBuf, PathBuf) {
    let server = echo_server();
    let dir = tmp(name);
    let (root, data) = (dir.join("collection"), dir.join("data"));
    collection(&root, server.port);
    (server, root, data)
}

#[test]
fn protocol_negotiation_ping_errors_and_batches() {
    let (_server, root, data) = setup("protocol");
    let mut mcp = Mcp::spawn(&root, &data, &[]);
    // Before `initialize`: refused, but ping works.
    let early = mcp.request("tools/list", json!({}));
    assert!(early["error"]["code"].is_i64(), "{early}");
    assert_eq!(mcp.request("ping", json!({}))["result"], json!({}));
    // An older version is accepted as asked; tools are announced.
    let init = mcp.request(
        "initialize",
        json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "old"}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert_eq!(init["result"]["serverInfo"]["name"], "requesttailor");
    mcp.notify("notifications/initialized", json!({}));
    // An unknown version gets the newest.
    let again = mcp.request(
        "initialize",
        json!({"protocolVersion": "1999-01-01", "capabilities": {}}),
    );
    assert_eq!(again["result"]["protocolVersion"], "2025-06-18");

    let unknown = mcp.request("resources/list", json!({}));
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");
    let bad_tool = mcp.request("tools/call", json!({"name": "rm_rf", "arguments": {}}));
    assert_eq!(bad_tool["error"]["code"], -32602, "{bad_tool}");
    let no_path = mcp.request(
        "tools/call",
        json!({"name": "send_request", "arguments": {}}),
    );
    assert_eq!(no_path["error"]["code"], -32602, "{no_path}");

    mcp.raw("{not json");
    let parse = mcp.read();
    assert_eq!(parse["error"]["code"], -32700, "{parse}");
    assert_eq!(parse["id"], Value::Null);

    mcp.raw(r#"[{"jsonrpc":"2.0","id":"a","method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":"b","method":"tools/list"}]"#);
    let batch = mcp.read();
    let replies = batch.as_array().expect("a batch is answered with an array");
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["id"], "a");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    let send = tools.iter().find(|t| t["name"] == "send_request").unwrap();
    assert_eq!(send["annotations"]["readOnlyHint"], false);
    assert_eq!(send["annotations"]["destructiveHint"], true);
    let list = tools.iter().find(|t| t["name"] == "list_requests").unwrap();
    assert_eq!(list["annotations"]["readOnlyHint"], true);
}

#[test]
#[cfg(unix)]
fn secrets_never_reach_the_agent_even_when_the_server_echoes_them() {
    let (server, root, data) = setup("echo");
    // `apikey` comes from a secret manager in Local.
    fake_op(&data);
    write(
        &root.join("environments/Local.yml"),
        &format!(
            "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:{}\n  - secret: true\n    name: token\nexternalSecrets:\n  type: one-password\n  variables:\n    - name: apikey\n      secretReference: op://Dev/api/key\n",
            server.port
        ),
    );
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/list.yml", "environment": "Local"}),
    );
    assert_ne!(r["isError"], json!(true), "{r}");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    let report = json_text(&r);
    // The server got the token; the body that echoes it reaches the agent masked.
    let body = report["response"]["body"]["data"].as_str().unwrap();
    assert!(body.contains("Bearer ***"), "{body}");
    assert!(
        body.contains("X-Api-Key: ***") || body.contains("x-api-key: ***"),
        "{body}"
    );

    let envs = json_text(&mcp.call("list_environments", json!({})));
    let local = envs
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Local")
        .unwrap()
        .clone();
    assert_eq!(local["access"], "write");
    let vars = local["variables"].as_array().unwrap();
    let token = vars.iter().find(|v| v["name"] == "token").unwrap();
    assert_eq!(token["secret"], true);
    assert!(token.get("value").is_none());
    let base = vars.iter().find(|v| v["name"] == "base").unwrap();
    assert!(
        base["value"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );

    let spec = json_text(&mcp.call("get_request", json!({"path": "orders/list.yml"})));
    assert_eq!(spec["headers"][0]["value"], "Bearer {{token}}");
    assert_eq!(spec["effect"], "read");
    let missing = mcp.call("get_request", json!({"path": "nope.yml"}));
    assert_eq!(missing["isError"], true);
    mcp.assert_no_secrets();
    let audit = std::fs::read_to_string(data.join("agents/audit.jsonl")).unwrap();
    assert!(!audit.contains(SECRET) && !audit.contains(EXTERNAL));
    // No bodies or header values in the audit log.
    assert!(!audit.contains("Bearer"), "{audit}");
}

#[test]
fn one_audit_entry_per_tool_call() {
    let (_server, root, data) = setup("audit");
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    assert_eq!(audit(&data).len(), 1, "the connection itself");
    let calls = [
        ("list_requests", json!({})),
        ("list_environments", json!({})),
        ("get_request", json!({"path": "orders/create.yml"})),
        (
            "send_request",
            json!({"path": "orders/list.yml", "environment": "Local"}),
        ),
        (
            "send_request",
            json!({"path": "orders/create.yml", "environment": "Staging"}),
        ),
        (
            "run_folder",
            json!({"folder": "orders", "environment": "Local"}),
        ),
    ];
    for (n, (tool, args)) in calls.iter().enumerate() {
        mcp.call(tool, args.clone());
        mcp.request("ping", json!({}));
        mcp.request("tools/list", json!({}));
        let entries = audit(&data);
        assert_eq!(entries.len(), n + 2, "after {tool}: {entries:?}");
        let last = entries.last().unwrap();
        assert_eq!(last["tool"], *tool);
        assert_eq!(last["agent"], "test-agent");
    }
    let entries = audit(&data);
    assert_eq!(entries[0]["tool"], "initialize");
    assert_eq!(entries[4]["outcome"], "sent");
    assert_eq!(entries[4]["status"], 200);
    assert_eq!(entries[4]["method"], "GET");
    assert_eq!(entries[5]["outcome"], "refused");
    assert_eq!(entries[6]["outcome"], "sent");
    assert!(
        entries[6]["detail"]
            .as_str()
            .unwrap()
            .contains("orders/create.yml: POST 200"),
        "{}",
        entries[6]
    );
    mcp.assert_no_secrets();
}

#[test]
fn a_denied_production_write_sends_nothing() {
    let (server, root, data) = setup("deny");
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    let user = answer_first(&data, "deny");
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/create.yml", "environment": "PRD"}),
    );
    let pending = user.join().unwrap();
    assert_eq!(pending["agent"], "test-agent");
    assert_eq!(pending["method"], "POST");
    assert!(
        pending["url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("127.0.0.1:{}/orders", server.port)),
        "{pending}"
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(text(&r).contains("denied"), "{r}");
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
    let last = audit(&data).pop().unwrap();
    assert_eq!(last["outcome"], "denied");
    assert_eq!(rt(&["approvals", "--json"], &data).1.trim(), "[]");
    // A second answer to the same id fails.
    let id = pending["id"].as_str().unwrap();
    assert_ne!(rt(&["approve", id], &data).0, 0);
    mcp.assert_no_secrets();
}

#[test]
fn elicitation_asks_the_client_and_the_first_answer_wins() {
    let (server, root, data) = setup("elicit");
    let mut mcp = Mcp::start(&root, &data, json!({"elicitation": {}}), &[]);
    mcp.raw(
        &json!({"jsonrpc": "2.0", "id": 100, "method": "tools/call", "params": {"name": "send_request", "arguments": {"path": "orders/create.yml", "environment": "PRD"}}}).to_string(),
    );
    let ask = mcp.read();
    assert_eq!(ask["method"], "elicitation/create", "{ask}");
    assert!(ask["params"]["message"].as_str().unwrap().contains("POST"));
    // The approval is in the folder too, for the app.
    assert_eq!(
        serde_json::from_str::<Value>(&rt(&["approvals", "--json"], &data).1)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Ping still works while the call waits.
    mcp.raw(r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#);
    assert_eq!(mcp.read()["id"], "p");
    mcp.raw(
        &json!({"jsonrpc": "2.0", "id": ask["id"], "result": {"action": "accept", "content": {"approve": true}}}).to_string(),
    );
    let done = loop {
        let v = mcp.read();
        if v["id"] == 100 {
            break v;
        }
    };
    assert_ne!(done["result"]["isError"], true, "{done}");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    let last = audit(&data).pop().unwrap();
    assert_eq!(last["approved_by"], "elicitation");
    assert_eq!(rt(&["approvals", "--json"], &data).1.trim(), "[]");

    // Declined: nothing sent.
    mcp.raw(
        &json!({"jsonrpc": "2.0", "id": 101, "method": "tools/call", "params": {"name": "send_request", "arguments": {"path": "orders/create.yml", "environment": "PRD"}}}).to_string(),
    );
    let ask = mcp.read();
    mcp.raw(
        &json!({"jsonrpc": "2.0", "id": ask["id"], "result": {"action": "decline"}}).to_string(),
    );
    let done = loop {
        let v = mcp.read();
        if v["id"] == 101 {
            break v;
        }
    };
    assert_eq!(done["result"]["isError"], true, "{done}");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);

    // Answered in the app (file) first: the open elicitation is cancelled.
    mcp.raw(
        &json!({"jsonrpc": "2.0", "id": 102, "method": "tools/call", "params": {"name": "send_request", "arguments": {"path": "orders/create.yml", "environment": "PRD"}}}).to_string(),
    );
    let ask = mcp.read();
    assert_eq!(ask["method"], "elicitation/create");
    answer_first(&data, "approve").join().unwrap();
    let mut cancelled = false;
    let done = loop {
        let v = mcp.read();
        if v["method"] == "notifications/cancelled" {
            assert_eq!(v["params"]["requestId"], ask["id"]);
            cancelled = true;
        }
        if v["id"] == 102 {
            break v;
        }
    };
    assert!(cancelled);
    assert_ne!(done["result"]["isError"], true, "{done}");
    assert_eq!(audit(&data).pop().unwrap()["approved_by"], "cli");
    mcp.assert_no_secrets();
}

#[test]
fn run_folder_checks_every_request() {
    let (server, root, data) = setup("run");
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    // Read-only environment: the GET goes out, the POST is refused.
    let r = mcp.call(
        "run_folder",
        json!({"folder": "orders", "environment": "Staging"}),
    );
    let run = json_text(&r);
    assert_eq!(run["summary"]["requests"], 2, "{r}");
    assert_eq!(*server.methods.lock().unwrap(), ["GET"]);
    let refused = &run["steps"][1]["report"]["error"]["message"];
    assert!(
        refused.to_string().contains("only reading"),
        "{}",
        run["steps"][1]
    );
    // Production: the POST waits for approval, then both go out.
    let user = answer_first(&data, "approve");
    let r = mcp.call("run_folder", json!({"environment": "PRD"}));
    let pending = user.join().unwrap();
    assert_eq!(pending["request_path"], "orders/create.yml");
    let run = json_text(&r);
    assert_eq!(run["summary"]["passed"], 2, "{r}");
    assert_eq!(*server.methods.lock().unwrap(), ["GET", "GET", "POST"]);
    let last = audit(&data).pop().unwrap();
    assert_eq!(last["tool"], "run_folder");
    assert_eq!(last["approved_by"], "cli");
    mcp.assert_no_secrets();
}

#[test]
fn agent_variables_cannot_redirect_or_replace_secrets() {
    let (server, root, data) = setup("vars");
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    for vars in [
        json!({"base": "http://evil.example"}),
        json!({"token": "mine"}),
        json!({"apikey": "mine"}),
    ] {
        let r = mcp.call(
            "send_request",
            json!({"path": "orders/list.yml", "environment": "Local", "vars": vars}),
        );
        assert_eq!(r["isError"], true, "{vars}: {r}");
    }
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
    // Request variables are fine.
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/list.yml", "environment": "Local", "vars": {"page": 7}}),
    );
    assert_ne!(r["isError"], true, "{r}");
    assert!(
        json_text(&r)["request"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/orders?page=7")
    );
    mcp.assert_no_secrets();
}

#[test]
fn the_policy_is_pinned_when_the_server_starts() {
    let (server, root, data) = setup("pinned");
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    // The agent loosens the file: PRD no longer production, Staging writable.
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n\nextensions:\n  requesttailor:\n    agents:\n      access: write\n",
    );
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/create.yml", "environment": "Staging"}),
    );
    assert_eq!(r["isError"], true, "{r}");
    // The user tightens it: applies at once.
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n\nextensions:\n  requesttailor:\n    agents:\n      access: none\n",
    );
    let r = mcp.call(
        "send_request",
        json!({"path": "orders/list.yml", "environment": "Local"}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert!(
        json_text(&mcp.call("list_environments", json!({})))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
}

#[test]
fn scripts_run_only_in_trusted_collections() {
    let (_server, root, data) = setup("trust");
    let with_test = "info:\n  name: Scripted\n  type: http\n  seq: 3\n\nhttp:\n  method: GET\n  url: \"{{base}}/s\"\n\nruntime:\n  scripts:\n    - type: tests\n      code: |-\n        test(\"ran\", () => {});\n";
    write(&root.join("orders/scripted.yml"), with_test);
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    let r = json_text(&mcp.call(
        "send_request",
        json!({"path": "orders/scripted.yml", "environment": "Local"}),
    ));
    assert!(r.get("tests").is_none(), "{r}");
    assert!(r["notes"].to_string().contains("not trusted"), "{r}");
    drop(mcp);

    // The id `rt` trusts by (no `\\?\` prefix on Windows).
    let id = rt_cli::oauth::collection_id(&root);
    write(
        &data.join("trusted-scripts.json"),
        &json!({"version": 1, "trusted": [id]}).to_string(),
    );
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    let r = json_text(&mcp.call(
        "send_request",
        json!({"path": "orders/scripted.yml", "environment": "Local"}),
    ));
    assert_eq!(r["tests"][0]["name"], "ran", "{r}");
    drop(mcp);

    // --no-scripts wins over trust.
    let mut mcp = Mcp::start(&root, &data, json!({}), &["--no-scripts"]);
    let r = json_text(&mcp.call(
        "send_request",
        json!({"path": "orders/scripted.yml", "environment": "Local"}),
    ));
    assert!(r.get("tests").is_none(), "{r}");
}

#[test]
fn a_cancelled_call_withdraws_its_approval() {
    let (server, root, data) = setup("cancel");
    let mut mcp = Mcp::start(&root, &data, json!({}), &[]);
    mcp.raw(
        &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "send_request", "arguments": {"path": "orders/create.yml", "environment": "PRD"}}}).to_string(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while rt(&["approvals", "--json"], &data).1.trim() == "[]" {
        assert!(Instant::now() < deadline, "no approval appeared");
        std::thread::sleep(Duration::from_millis(50));
    }
    mcp.notify("notifications/cancelled", json!({"requestId": 7}));
    let done = loop {
        let v = mcp.read();
        if v["id"] == 7 {
            break v;
        }
    };
    assert_eq!(done["result"]["isError"], true, "{done}");
    assert_eq!(rt(&["approvals", "--json"], &data).1.trim(), "[]");
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
}
