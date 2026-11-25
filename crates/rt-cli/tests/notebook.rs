// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand W: `rt notebook run`. A tiny loopback server answers a login and an order
//! list; the notebook passes the token from the first block to the second, prints tables and
//! never a secret, skips writing blocks without `--allow-writes`, refuses production, exits 4
//! for a failing block, and saves nothing.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const SECRET: &str = "RT-CANARY-notebook-cli-4c1e";
const TOKEN: &str = "RT-CANARY-notebook-cli-token-9a07";

struct Server {
    port: u16,
    hits: Arc<AtomicUsize>,
}

fn server() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            counter.fetch_add(1, Ordering::SeqCst);
            let bearer = head.contains(&format!("Bearer {TOKEN}"))
                || head.contains(&format!("bearer {TOKEN}"));
            let (status, body) = if head.starts_with("GET /login") {
                (
                    200,
                    format!(
                        "{{\"access_token\":\"{TOKEN}\",\"user\":{{\"id\":42}},\"echo\":\"{SECRET}\"}}"
                    ),
                )
            } else if head.starts_with("GET /orders") && bearer {
                (
                    200,
                    "{\"items\":[{\"id\":1,\"total\":10.5},{\"id\":2,\"total\":4}]}".to_owned(),
                )
            } else if head.starts_with("GET /orders") {
                (401, "{\"error\":\"no token\"}".to_owned())
            } else if head.starts_with("POST /orders") {
                (201, "{\"created\":true}".to_owned())
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
    let dir = std::env::temp_dir().join(format!("rt-notebook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const NOTEBOOK: &str = "# Orders\n\nLog in.\n\n```rt-request\nid: login\nmethod: GET\nurl: {{base}}/login\nheaders:\n  x-key: {{apiKey}}\nset:\n  token: $.access_token\n  userId: $.user.id\nshow:\n  - response\n```\n\nThen the orders.\n\n```rt-request\nid: list\nmethod: GET\nurl: {{base}}/orders\nheaders:\n  Authorization: Bearer {{token}}\nshow:\n  - table: $.items[*]\n```\n\n```rt-request\nid: create\nmethod: POST\nurl: {{base}}/orders\nbody: {\"a\":1}\n```\n";

fn collection(root: &Path, port: u16) -> PathBuf {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Orders\n",
    );
    for env in ["Test", "PRD"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: apiKey\n"
            ),
        );
    }
    let file = root.join("orders.notebook.md");
    write(&file, NOTEBOOK);
    file
}

fn rt(dir: &Path, file: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["notebook", "run"])
        .arg(file)
        .args([
            "--var",
            &format!("apiKey={SECRET}"),
            "--no-plugins",
            "--no-scripts",
        ])
        .args(extra)
        .env("RT_DATA_DIR", dir.join("data"))
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn audit(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("data/agents/audit.jsonl")).unwrap_or_default()
}

fn clean(o: &Output, dir: &Path) {
    for text in [out(o), err(o), audit(dir)] {
        assert!(!text.contains(SECRET) && !text.contains(TOKEN), "{text}");
    }
}

#[test]
fn blocks_run_in_order_pass_the_token_on_and_print_a_table() {
    let srv = server();
    let dir = tmp("run");
    let file = collection(&dir.join("c"), srv.port);
    let o = rt(&dir, &file, &["-e", "Test"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    let text = out(&o);
    let (login, list) = (
        text.find("## login").unwrap(),
        text.find("## list").unwrap(),
    );
    assert!(login < list);
    assert!(text.contains("userId = 42"), "{text}");
    assert!(
        text.contains("token = ***"),
        "the credential is carried, not shown: {text}"
    );
    assert!(text.contains("200"), "{text}");
    assert!(
        text.contains("id\ttotal") && text.contains("1\t10.5"),
        "{text}"
    );
    // The writing block was skipped and said so; two requests went out.
    assert!(text.contains("## create  skipped (writes)"), "{text}");
    assert!(err(&o).contains("--allow-writes"), "{}", err(&o));
    assert_eq!(srv.hits.load(Ordering::SeqCst), 2);
    clean(&o, &dir);
    // Nothing was written: the notebook is unchanged and no output file exists.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), NOTEBOOK);
    let names: Vec<String> = std::fs::read_dir(dir.join("c"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writes_need_the_flag_and_never_run_against_production() {
    let srv = server();
    let dir = tmp("writes");
    let file = collection(&dir.join("c"), srv.port);
    let o = rt(&dir, &file, &["-e", "Test", "--allow-writes"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(out(&o).contains("201"), "{}", out(&o));
    assert_eq!(srv.hits.load(Ordering::SeqCst), 3);
    assert!(audit(&dir).contains("notebook-run"));
    let p = rt(&dir, &file, &["-e", "PRD", "--allow-writes"]);
    assert!(
        out(&p).contains("## create  skipped (production)"),
        "{}",
        out(&p)
    );
    assert!(err(&p).contains("PRODUCTION"), "{}", err(&p));
    assert_eq!(srv.hits.load(Ordering::SeqCst), 5, "two reads, no write");
    clean(&o, &dir);
    clean(&p, &dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_block_and_json_output() {
    let srv = server();
    let dir = tmp("json");
    let file = collection(&dir.join("c"), srv.port);
    let o = rt(&dir, &file, &["-e", "Test", "--block", "login", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["results"].as_array().unwrap().len(), 1);
    assert_eq!(v["results"][0]["block"], "login");
    assert_eq!(srv.hits.load(Ordering::SeqCst), 1);
    clean(&o, &dir);
    let bad = rt(&dir, &file, &["--block", "nope"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(err(&bad).contains("no block"), "{}", err(&bad));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_assertion_or_missing_request_exits_4() {
    let srv = server();
    let dir = tmp("fail");
    let file = collection(&dir.join("c"), srv.port);
    write(&file, "```rt-request\nid: a\nref: missing.yml\n```\n");
    let o = rt(&dir, &file, &["-e", "Test"]);
    assert_eq!(o.status.code(), Some(4), "{}{}", out(&o), err(&o));
    // A file that is not a notebook is a usage error.
    let other = dir.join("c/opencollection.yml");
    assert_eq!(rt(&dir, &other, &[]).status.code(), Some(2));
    // A notebook that does not parse says where.
    write(&file, "```rt-request\nfoo: 1\n```\n");
    let o = rt(&dir, &file, &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("unknown key"), "{}", err(&o));
    let _ = std::fs::remove_dir_all(&dir);
}
