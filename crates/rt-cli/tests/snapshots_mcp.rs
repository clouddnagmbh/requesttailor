// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests through `rt mcp` (wave 17, strand B): `send_request` and `run_folder` report
//! the snapshot result like any assertion (masked), and an agent has no way to freeze or
//! accept a snapshot: there is no such tool, and no file changes.

#[path = "support/snapshot_server.rs"]
mod server;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const SECRET: &str = "RT-CANARY-mcp-snapshot-1";

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    seen: Vec<String>,
    next_id: u64,
}

impl Mcp {
    fn start(root: &Path, data: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
            .args(["mcp", root.to_str().unwrap()])
            .args(["--var", &format!("token={SECRET}")])
            .env("RT_DATA_DIR", data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}});
        writeln!(mcp.stdin, "{note}").unwrap();
        mcp
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{line}").unwrap();
        loop {
            let mut text = String::new();
            assert!(
                self.stdout.read_line(&mut text).unwrap() > 0,
                "rt mcp closed"
            );
            self.seen.push(text.clone());
            let v: Value = serde_json::from_str(&text).unwrap();
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
    }
}

fn json_text(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or_default())
        .unwrap_or(Value::Null)
}

fn setup(port: u16) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rt-snapshot-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (root, data) = (dir.join("c"), dir.join("data"));
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Items\n\nextensions:\n  requesttailor:\n    agents:\n      access: read\n",
    );
    write(
        &root.join("environments/Local.yml"),
        &format!(
            "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
        ),
    );
    write(
        &root.join("items/folder.yml"),
        "info:\n  name: items\n  type: folder\n  seq: 1\n",
    );
    write(
        &root.join("items/get.yml"),
        "info:\n  name: Get item\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/item\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{token}}\"\n\nruntime:\n  assertions:\n    - expression: res\n      operator: matchesSnapshot\n      value: get.snap.json\n",
    );
    (dir, root, data)
}

fn files(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in walk(root) {
        out.push((
            entry
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            std::fs::read_to_string(&entry).unwrap(),
        ));
    }
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn the_agent_sees_the_result_and_cannot_freeze() {
    let srv = server::spawn();
    let (dir, root, data) = setup(srv.port);
    let mut mcp = Mcp::start(&root, &data);

    // There is no tool that writes a snapshot.
    let tools = mcp.request("tools/list", json!({}))["result"]["tools"].clone();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        !names
            .iter()
            .any(|n| n.contains("snapshot") || n.contains("freeze")),
        "{names:?}"
    );

    let before = files(&root);

    // No file yet: the assertion fails with the hint.
    let r = mcp.call(
        "send_request",
        json!({"path": "items/get.yml", "environment": "Local"}),
    );
    let report = json_text(&r);
    let a = &report["assertions"][0];
    assert_eq!(a["operator"], "matchesSnapshot", "{r}");
    assert_eq!(a["passed"], false);
    assert_eq!(a["error"], "no snapshot yet; run `rt snapshot update`");

    // The user freezes it with the command; the agent then sees green (after the ignore) or
    // the difference.
    let snap = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args([
            "snapshot",
            "update",
            root.join("items/get.yml").to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            &format!("token={SECRET}"),
        ])
        .env("RT_DATA_DIR", &data)
        .status()
        .unwrap();
    assert!(snap.success());
    let file = root.join("items/get.snap.json");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        text.replace("\"ignore\": []", "\"ignore\": [\"$.stamp\"]"),
    )
    .unwrap();
    let frozen = files(&root);
    let r = mcp.call(
        "send_request",
        json!({"path": "items/get.yml", "environment": "Local"}),
    );
    assert_eq!(json_text(&r)["assertions"][0]["passed"], true, "{r}");
    *srv.name.lock().unwrap() = "coffee".into();
    let r = mcp.call(
        "run_folder",
        json!({"folder": "items", "environment": "Local"}),
    );
    let run = json_text(&r);
    assert_eq!(run["summary"]["passed"], 0, "{r}");
    let error = run["steps"][0]["report"]["assertions"][0]["error"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(error.contains("body.name"), "{error}");

    // Nothing the agent did changed a file; nothing it saw has the secret.
    assert_eq!(files(&root), frozen);
    assert_ne!(before, frozen);
    for line in &mcp.seen {
        assert!(
            !line.contains(SECRET),
            "secret in the agent context: {line}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
