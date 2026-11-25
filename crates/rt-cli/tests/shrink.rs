// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand E: `rt shrink`. A tiny server answers 500 when the request line holds
//! `bad=1`; the command must keep that and take the rest away, exit 0, print what was removed
//! and the smallest request as `curl`, and never print a secret value.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rt_core::guard::{GuardHandle, GuardVerdict, SendCheck, SendGuard};

const SECRET: &str = "RT-CANARY-shrink-cli-8c41";

struct Server {
    port: u16,
    hits: Arc<AtomicUsize>,
}

/// Answers 500 when the head contains `needle`, else 200.
fn server(needle: &'static str) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            counter.fetch_add(1, Ordering::SeqCst);
            let status = if head.contains(needle) {
                "500 Internal Server Error"
            } else {
                "200 OK"
            };
            let body = format!("{{\"echo\":\"{}\"}}", head.lines().next().unwrap_or(""));
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Server { port, hits }
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-shrink-{name}-{}", std::process::id()));
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
        "opencollection: \"1.0.0\"\ninfo:\n  name: Shrink\n",
    );
    for env in ["Local", "PRD"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
            ),
        );
    }
    write(
        &root.join("get.yml"),
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/items?a=1&bad=1&c=3\"\n  headers:\n    - name: X-A\n      value: one\n    - name: X-Debug\n      value: on\n    - name: Authorization\n      value: Bearer {{token}}\n",
    );
    write(
        &root.join("post.yml"),
        "info:\n  name: Post\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{base}}/items?bad=1\"\n  body:\n    type: json\n    data: '{\"a\":1,\"b\":2}'\n",
    );
}

fn rt(args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rt_cli::run(args.iter().copied().map(String::from), &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn shrink_args<'a>(file: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "shrink",
        file,
        "--env",
        "Local",
        "--var",
        const_secret(),
        "--no-plugins",
    ];
    args.extend_from_slice(extra);
    args
}

fn const_secret() -> &'static str {
    // `--var NAME=VALUE` (leaked once: the test binary is short-lived).
    Box::leak(format!("token={SECRET}").into_boxed_str())
}

#[test]
fn the_smallest_request_is_printed_and_nothing_secret_is() {
    let srv = server("bad=1");
    let dir = tmp("find");
    collection(&dir, srv.port);
    let file = dir.join("get.yml");
    let (code, out, err) = rt(&shrink_args(
        file.to_str().unwrap(),
        &["--predicate", "status=500"],
    ));
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("removed query:a"), "{out}");
    assert!(out.contains("removed query:c"), "{out}");
    assert!(out.contains("removed header:X-A"), "{out}");
    assert!(out.contains("removed header:X-Debug"), "{out}");
    assert!(!out.contains("removed header:Authorization"), "{out}");
    assert!(out.contains("1-minimal"), "{out}");
    // The curl command keeps the cause and the (variable) credentials, nothing else.
    let curl = out.lines().find(|l| l.contains("curl")).unwrap_or_default();
    assert!(curl.contains("bad=1"), "{out}");
    assert!(!curl.contains("a=1&"), "{out}");
    assert!(!curl.contains("X-Debug"), "{out}");
    for text in [&out, &err] {
        assert!(!text.contains(SECRET), "{text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_output_lists_parts_without_values() {
    let srv = server("bad=1");
    let dir = tmp("json");
    collection(&dir, srv.port);
    let file = dir.join("get.yml");
    let (code, out, _) = rt(&shrink_args(file.to_str().unwrap(), &["--json"]));
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let removed: Vec<&str> = v["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["part"].as_str().unwrap())
        .collect();
    assert_eq!(
        removed,
        ["header:X-A", "header:X-Debug", "query:a", "query:c"]
    );
    assert_eq!(v["one_minimal"], true);
    assert!(v["probes"].as_u64().unwrap() <= 20);
    assert!(!out.contains(SECRET));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keep_and_probe_limit_are_honoured() {
    let srv = server("bad=1");
    let dir = tmp("keep");
    collection(&dir, srv.port);
    let file = dir.join("get.yml");
    let (code, out, _) = rt(&shrink_args(
        file.to_str().unwrap(),
        &["--json", "--keep", "header:X-Debug", "--keep", "query:c"],
    ));
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let removed: Vec<&str> = v["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["part"].as_str().unwrap())
        .collect();
    assert_eq!(removed, ["header:X-A", "query:a"]);

    let (code, out, _) = rt(&shrink_args(
        file.to_str().unwrap(),
        &["--json", "--max-probes", "3"],
    ));
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["stopped_by"], "probes");
    assert_eq!(v["probes"], 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failure_that_does_not_reproduce_exits_1() {
    let srv = server("never-in-the-request");
    let dir = tmp("none");
    collection(&dir, srv.port);
    let file = dir.join("get.yml");
    let (code, out, _) = rt(&shrink_args(
        file.to_str().unwrap(),
        &["--predicate", "status=500"],
    ));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("did not reproduce"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writing_requests_need_allow_writes_and_never_run_against_production() {
    let srv = server("bad=1");
    let dir = tmp("writes");
    collection(&dir, srv.port);
    let file = dir.join("post.yml");
    let (code, _, err) = rt(&shrink_args(file.to_str().unwrap(), &[]));
    assert_eq!(code, 2);
    assert!(err.contains("requests that write"), "{err}");
    assert_eq!(srv.hits.load(Ordering::SeqCst), 0);

    let (code, out, _) = rt(&shrink_args(
        file.to_str().unwrap(),
        &["--allow-writes", "--json"],
    ));
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["one_minimal"], true);
    let before = srv.hits.load(Ordering::SeqCst);
    assert!(before > 0);

    let args = vec![
        "shrink",
        file.to_str().unwrap(),
        "--env",
        "PRD",
        "--var",
        const_secret(),
        "--no-plugins",
        "--allow-writes",
    ];
    let (code, _, err) = rt(&args);
    assert_eq!(code, 2);
    assert!(err.contains("production"), "{err}");
    assert_eq!(
        srv.hits.load(Ordering::SeqCst),
        before,
        "nothing went to production"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

struct Deny;

impl SendGuard for Deny {
    fn check(&self, check: &SendCheck) -> GuardVerdict {
        assert!(!check.url.contains(SECRET), "the guard saw a secret");
        GuardVerdict::Refuse {
            rule: "cli.no-shrink".into(),
            reason: "no".into(),
        }
    }
}

#[test]
fn a_guard_refusal_exits_with_blocked_and_sends_nothing() {
    let srv = server("bad=1");
    let dir = tmp("guard");
    collection(&dir, srv.port);
    let file = dir.join("get.yml");
    let args = shrink_args(file.to_str().unwrap(), &[]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rt_cli::run_distribution(
        args.iter().copied().map(String::from),
        &mut out,
        &mut err,
        &rt_cli::Distribution {
            guard: GuardHandle::new(Arc::new(Deny)),
            ..Default::default()
        },
    );
    assert_eq!(
        code,
        rt_cli::EXIT_BLOCKED,
        "{}",
        String::from_utf8_lossy(&err)
    );
    assert!(String::from_utf8_lossy(&err).contains("cli.no-shrink"));
    assert_eq!(srv.hits.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn predicates_are_parsed_and_bad_ones_refused() {
    use rt_app::shrink::Predicate;
    use rt_cli::shrink::parse_predicate;
    assert_eq!(
        parse_predicate("status=500"),
        Ok(Predicate::Status { status: 500 })
    );
    assert_eq!(parse_predicate("5xx"), Ok(Predicate::ServerError));
    assert_eq!(parse_predicate("last"), Ok(Predicate::LastFailure));
    assert!(parse_predicate("status=abc").is_err());
    assert!(parse_predicate("nonsense").is_err());
    let (code, _, err) = rt(&["shrink", "x.yml", "--predicate", "nonsense"]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown predicate"), "{err}");
}
