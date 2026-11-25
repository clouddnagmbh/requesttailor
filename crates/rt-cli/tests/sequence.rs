// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand G: `--sequence <file>` of `rt send`, `rt run` and `rt mcp`. The diagram is
//! written atomically when the command ends (an existing file is replaced whole, no temporary
//! file stays), shows what really went out, and holds no secret or value unless
//! `--sequence-values` asks for short masked excerpts.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const SECRET: &str = "RT-CANARY-sequence-cli-secret";
const PERSONAL: &str = "carol.canary@example.test";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-sequence-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A server answering JSON with a personal value and a secret-named field.
fn server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = vec![0u8; 65536];
            let _ = stream.read(&mut buf);
            let body = format!("{{\"id\":7,\"email\":\"{PERSONAL}\",\"api_token\":\"{SECRET}\"}}");
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, port: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Items\n",
    );
    write(
        &root.join("environments/Local.yml"),
        &format!(
            "name: Local\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
        ),
    );
    for (n, name) in [(1, "first"), (2, "second")] {
        write(
            &root.join(format!("items/{name}.yml")),
            &format!(
                "info:\n  name: {name}\n  type: http\n  seq: {n}\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/items/{n}?token={{{{token}}}}\"\n"
            ),
        );
    }
}

fn rt(args: &[&str], data: &Path) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(args)
        .args(["--no-plugins", "--var", &format!("token={SECRET}")])
        .env("RT_DATA_DIR", data)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let port = server();
    let dir = tmp(name);
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, port);
    (dir, root, data)
}

/// One JSON-RPC call: writes the line, reads until the answer with `id`.
fn rpc(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let line = if id == 0 {
        json!({"jsonrpc": "2.0", "method": method, "params": params})
    } else {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    };
    writeln!(stdin, "{line}").unwrap();
    if id == 0 {
        return Value::Null;
    }
    loop {
        let mut text = String::new();
        assert!(
            stdout.read_line(&mut text).unwrap() > 0,
            "rt mcp closed stdout"
        );
        let v: Value = serde_json::from_str(&text).unwrap();
        if v["id"] == json!(id) {
            return v;
        }
    }
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[test]
fn send_writes_the_diagram_atomically_without_secrets_or_values() {
    let (dir, root, data) = setup("send");
    let out = dir.join("diagram.md");
    std::fs::write(&out, "OLD CONTENT").unwrap();
    let file = root.join("items/first.yml");
    let (code, _, err) = rt(
        &[
            "send",
            file.to_str().unwrap(),
            "-e",
            "Local",
            "--sequence",
            out.to_str().unwrap(),
        ],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("```mermaid\nsequenceDiagram\n"), "{text}");
    assert!(text.trim_end().ends_with("```"), "{text}");
    assert!(!text.contains("OLD CONTENT"));
    assert!(text.contains("participant P1 as 127.0.0.1#58;"), "{text}");
    assert!(text.contains("GET /items/{id}"), "{text}");
    assert!(text.contains("200 ("), "{text}");
    for forbidden in [SECRET, PERSONAL, "token=", "api_token"] {
        assert!(!text.contains(forbidden), "{forbidden} in {text}");
    }
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
}

#[test]
fn values_only_with_the_switch_and_never_secrets() {
    let (dir, root, data) = setup("values");
    let out = dir.join("diagram.md");
    let file = root.join("items/first.yml");
    let (code, _, err) = rt(
        &[
            "send",
            file.to_str().unwrap(),
            "-e",
            "Local",
            "--sequence",
            out.to_str().unwrap(),
            "--sequence-values",
        ],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains(PERSONAL), "{text}");
    assert!(text.contains("id=7"), "{text}");
    assert!(!text.contains(SECRET), "{text}");
    assert!(!text.contains("api_token"), "{text}");
    // The switch alone is a mistake.
    let (code, _, err) = rt(
        &["send", file.to_str().unwrap(), "--sequence-values"],
        &data,
    );
    assert_eq!(code, 2);
    assert!(err.contains("--sequence-values needs --sequence"), "{err}");
}

#[test]
fn run_writes_one_diagram_of_all_requests() {
    let (dir, root, data) = setup("run");
    let out = dir.join("run.md");
    let (code, _, err) = rt(
        &[
            "run",
            root.to_str().unwrap(),
            "-e",
            "Local",
            "--sequence",
            out.to_str().unwrap(),
        ],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert_eq!(text.matches("P0->>P1").count(), 2, "{text}");
    assert_eq!(text.matches("P1-->>P0").count(), 2, "{text}");
    assert!(!text.contains(SECRET));
    assert!(leftovers(&dir).is_empty());
}

#[test]
fn a_failed_start_leaves_an_existing_file_alone() {
    let dir = tmp("failed");
    let out = dir.join("diagram.md");
    std::fs::write(&out, "KEEP").unwrap();
    let (code, _, _) = rt(
        &[
            "run",
            dir.join("nope").to_str().unwrap(),
            "--sequence",
            out.to_str().unwrap(),
        ],
        &dir,
    );
    assert_ne!(code, 0);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "KEEP");
    // A folder is no file name.
    let (code, _, err) = rt(&["run", ".", "--sequence", dir.to_str().unwrap()], &dir);
    assert_eq!(code, 2);
    assert!(err.contains("folder"), "{err}");
}

#[test]
fn mcp_writes_the_session_when_it_ends_with_an_agent_participant() {
    let (dir, root, data) = setup("mcp");
    let out = dir.join("mcp.md");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args([
            "mcp",
            root.to_str().unwrap(),
            "--sequence",
            out.to_str().unwrap(),
        ])
        .args(["--var", &format!("token={SECRET}")])
        .env("RT_DATA_DIR", &data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask =
        |id: u64, method: &str, params: Value| rpc(&mut stdin, &mut stdout, id, method, params);
    let init = ask(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    ask(0, "notifications/initialized", json!({}));
    let sent = ask(
        2,
        "tools/call",
        json!({"name": "send_request", "arguments": {"path": "items/first.yml", "environment": "Local"}}),
    );
    assert_ne!(sent["result"]["isError"], json!(true), "{sent}");
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("participant P0 as Agent"), "{text}");
    assert!(text.contains("send_request items/first.yml"), "{text}");
    assert!(text.contains("GET /items/{id}"), "{text}");
    assert!(!text.contains(SECRET), "{text}");
    assert!(leftovers(&dir).is_empty());
}
