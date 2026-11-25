// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --view` and `--view-expr` (wave 18, strand G) on the built binary: a saved view of
//! `<request>.views.json` or an expression picks from the response and prints that instead of
//! the report (json, text, table), the exit code stays the response's, a secret value echoed by
//! the server stays masked, and a bad name or expression is a usage error.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

const CANARY: &str = "RT-CANARY-views-cli-9";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const VIEWS: &str = r#"{
  "version": 1,
  "views": [
    { "name": "Ids", "language": "jsonpath", "expression": "$.items[*].id", "display": "json" },
    { "name": "Names", "language": "jsonpath", "expression": "$.items[*].name", "display": "text" },
    { "name": "Users", "language": "jsonpath", "expression": "$.items", "display": "table", "columns": ["id", "name"] },
    { "name": "Echo", "language": "jsonpath", "expression": "$.echo", "display": "text" },
    { "name": "Jq", "language": "jq", "expression": ".items", "display": "json" }
  ]
}
"#;

fn collection(port: u16) -> Dir {
    let dir = std::env::temp_dir().join(format!("rt-views-cli-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("environments")).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: shop\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("environments/dev.yml"),
        "name: dev\nvariables:\n  - secret: true\n    name: token\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("get.yml"),
        format!(
            "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://127.0.0.1:{port}/x?t={{{{token}}}}\"\n  auth: inherit\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("get.views.json"), VIEWS).unwrap();
    Dir(dir)
}

/// A server that answers every request with a JSON body that also echoes the query string.
fn serve(status: &str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let status = status.to_owned();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]);
            let query = head
                .split_whitespace()
                .nth(1)
                .and_then(|t| t.split_once('?'))
                .map(|(_, q)| q.to_owned())
                .unwrap_or_default();
            let body = format!(
                r#"{{"items":[{{"id":1,"name":"Ada"}},{{"id":2,"name":"Bob","extra":true}}],"echo":"{query}"}}"#
            );
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    port
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn send(dir: &Dir, flags: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("send")
        .arg(dir.0.join("get.yml"))
        .args(["-e", "dev", "--var"])
        .arg(format!("token={CANARY}"))
        .args(flags)
        .env("RT_DATA_DIR", std::env::temp_dir().join("rt-views-data"))
        .output()
        .expect("run rt");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn a_saved_view_prints_what_it_picks_instead_of_the_report() {
    let dir = collection(serve("200 OK"));
    let out = send(&dir, &["--view", "Ids"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let value: serde_json::Value = serde_json::from_str(&out.stdout).expect("JSON");
    assert_eq!(value, serde_json::json!([1, 2]));
    assert!(!out.stdout.contains("\"status\""), "not the report");

    let out = send(&dir, &["--view", "Names"]);
    assert_eq!(out.stdout, "Ada\nBob\n");

    let out = send(&dir, &["--view", "Users"]);
    assert_eq!(out.stdout, "id\tname\n1\tAda\n2\tBob\n");
}

#[test]
fn an_expression_of_your_own_and_the_exit_code() {
    let dir = collection(serve("404 Not Found"));
    let mut request = std::fs::read_to_string(dir.0.join("get.yml")).unwrap();
    request.push_str(
        "\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n",
    );
    std::fs::write(dir.0.join("get.yml"), request).unwrap();
    let out = send(&dir, &["--view-expr", "$.items[0].name"]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&out.stdout).unwrap(),
        serde_json::json!(["Ada"])
    );
    assert_eq!(
        out.code, 4,
        "the exit code stays the response's: {}",
        out.stderr
    );
    // No match prints an empty list, not an error.
    let out = send(&dir, &["--view-expr", "$.nothing"]);
    assert_eq!(out.stdout.trim(), "[]");
}

#[test]
fn a_secret_the_server_echoes_stays_masked() {
    let dir = collection(serve("200 OK"));
    let out = send(&dir, &["--view", "Echo"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(!out.stdout.contains(CANARY), "{}", out.stdout);
    assert!(!out.stderr.contains(CANARY));
    assert!(out.stdout.contains("***"), "{}", out.stdout);
}

#[test]
fn wrong_use_is_a_usage_error() {
    let dir = collection(serve("200 OK"));
    for (flags, needle) in [
        (vec!["--view", "Nope"], "Nope"),
        (vec!["--view", "Jq"], "jq is not available in this build"),
        (vec!["--view-expr", "$.["], "invalid JSONPath"),
        (vec!["--view", "Ids", "--view-expr", "$"], "not both"),
    ] {
        let out = send(&dir, &flags);
        assert_eq!(out.code, 2, "{flags:?}: {}{}", out.stdout, out.stderr);
        assert!(out.stderr.contains(needle), "{flags:?}: {}", out.stderr);
        assert!(!out.stdout.contains(CANARY));
    }
}

#[test]
fn without_the_flags_the_report_is_unchanged() {
    let dir = collection(serve("200 OK"));
    let out = send(&dir, &[]);
    let report: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(report["response"]["status"], 200);
}
