// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand F: `rt verify-consumers`. Two local consumers and one cloned consumer run
//! against a tiny in-process provider with a deliberate change (the price becomes text): the
//! breaking consumer and request are named, exit 8, `--out json` and `--out github`, writing
//! requests only with `--allow-writes`, a send guard that refuses (exit 5), usage errors
//! (exit 2), and never a secret or a personal value in the output.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rt_app::consumers::ConsumerReport;
use rt_core::guard::{GuardHandle, GuardVerdict, SendCheck, SendGuard};

const PERSONAL: &str = "erin.canary@example.test";

struct Provider {
    base: String,
    hits: Arc<Mutex<Vec<String>>>,
    changed: Arc<AtomicBool>,
}

fn provider() -> Provider {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let hits = Arc::new(Mutex::new(Vec::new()));
    let changed = Arc::new(AtomicBool::new(false));
    let (h, c) = (Arc::clone(&hits), Arc::clone(&changed));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let (h, c) = (Arc::clone(&h), Arc::clone(&c));
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let mut parts = head.lines().next().unwrap_or("").split(' ');
                let method = parts.next().unwrap_or("").to_owned();
                let path = parts
                    .next()
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                h.lock().unwrap().push(format!("{method} {path}"));
                let (status, body) = match (method.as_str(), path.as_str()) {
                    ("GET", "/orders/1") if c.load(Ordering::SeqCst) => (
                        200,
                        format!("{{\"id\":1,\"items\":[{{\"price\":\"{PERSONAL}\"}}]}}"),
                    ),
                    ("GET", "/orders/1") => {
                        (200, "{\"id\":1,\"items\":[{\"price\":12.5}]}".to_owned())
                    }
                    ("GET", "/orders") => (200, "{\"count\":1}".to_owned()),
                    ("POST", "/orders") => (201, "{\"id\":2}".to_owned()),
                    ("GET", "/health") => (200, "{\"ok\":true}".to_owned()),
                    _ => (404, "{}".to_owned()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    Provider {
        base,
        hits,
        changed,
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/consumers")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.test"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rt-verify-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The provider's collection and copies of the two local consumers next to it; the third
/// consumer is cloned from a repository made here.
fn setup(tmp: &Path, with_clone: bool) -> PathBuf {
    let mut entries = String::from(
        "      - name: billing\n        source: ../billing\n        environment: ci\n      - name: portal\n        source: ../portal\n        environment: ci\n        vars: { baseUrl: \"{{provider.baseUrl}}\" }\n",
    );
    for c in ["billing", "portal"] {
        copy_dir(&fixtures().join(c), &tmp.join(c));
    }
    if with_clone {
        let repo = tmp.join("mobile-repo");
        copy_dir(&fixtures().join("mobile"), &repo);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "one"]);
        entries.push_str(&format!(
            "      - name: mobile\n        source: {}\n        environment: ci\n",
            file_url(&repo)
        ));
    }
    let root = tmp.join("provider");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        format!(
            "opencollection: 1.0.0\ninfo:\n  name: orders provider\nextensions:\n  requesttailor:\n    consumers:\n{entries}"
        ),
    )
    .unwrap();
    root
}

fn rt(tmp: &Path, root: &Path, base: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("verify-consumers")
        .arg(root)
        .args(["--base-url", base])
        .args(extra)
        .env("RT_DATA_DIR", tmp.join("data"))
        .env_remove("RT_GIT_TOKEN")
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn a_breaking_consumer_is_named_and_exits_8() {
    let tmp = Temp::new("break");
    let root = setup(&tmp.0, true);
    let p = provider();
    p.changed.store(true, Ordering::SeqCst);
    let o = rt(&tmp.0, &root, &p.base, &[]);
    assert_eq!(o.status.code(), Some(rt_cli::EXIT_CONSUMERS), "{}", err(&o));
    let text = out(&o);
    assert!(text.contains("Consumer billing breaks"), "{text}");
    assert!(text.contains("GET /orders/{orderId}"), "{text}");
    assert!(text.contains("Consumer portal is fine"), "{text}");
    assert!(text.contains("Consumer mobile is fine"), "{text}");
    assert!(!text.contains(PERSONAL) && !err(&o).contains(PERSONAL));
    // The clone is shallow and lives in the data folder.
    let clone = std::fs::read_dir(tmp.0.join("data/consumers"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(clone.join(".git/shallow").is_file() || clone.join(".git").is_dir());
}

#[test]
fn json_is_the_report_and_github_prints_annotations() {
    let tmp = Temp::new("formats");
    let root = setup(&tmp.0, false);
    let p = provider();
    p.changed.store(true, Ordering::SeqCst);
    let o = rt(&tmp.0, &root, &p.base, &["--out", "json"]);
    assert_eq!(o.status.code(), Some(8));
    let report: ConsumerReport = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(report.consumers.len(), 2);
    assert_eq!(
        report.consumers[0].failures[0].request,
        "GET /orders/{orderId}"
    );
    assert!(!out(&o).contains(PERSONAL));

    let o = rt(&tmp.0, &root, &p.base, &["--out", "github"]);
    assert_eq!(o.status.code(), Some(8));
    let text = out(&o);
    assert!(
        text.starts_with("::error title=Consumer billing breaks%3A GET /orders/{orderId}::"),
        "{text}"
    );
    assert_eq!(text.lines().count(), 1, "{text}");
}

#[test]
fn an_unchanged_provider_exits_0() {
    let tmp = Temp::new("green");
    let root = setup(&tmp.0, false);
    let p = provider();
    let o = rt(&tmp.0, &root, &format!("{}/", p.base), &[]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(out(&o).contains("0 of 2 consumers broken"), "{}", out(&o));
    let o = rt(&tmp.0, &root, &p.base, &["--consumer", "portal"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(!out(&o).contains("billing"));
}

#[test]
fn writing_requests_need_allow_writes() {
    let tmp = Temp::new("writes");
    let root = setup(&tmp.0, false);
    let p = provider();
    let o = rt(&tmp.0, &root, &p.base, &["--consumer", "billing"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(!p.hits.lock().unwrap().iter().any(|h| h.starts_with("POST")));
    assert!(out(&o).contains("1 left out"), "{}", out(&o));
    let o = rt(
        &tmp.0,
        &root,
        &p.base,
        &["--consumer", "billing", "--allow-writes"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert_eq!(
        p.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.starts_with("POST"))
            .count(),
        1
    );
}

struct Refuse;

impl SendGuard for Refuse {
    fn check(&self, _: &SendCheck) -> GuardVerdict {
        GuardVerdict::Refuse {
            rule: "test.no-consumers".into(),
            reason: "not today".into(),
        }
    }
}

#[test]
fn a_refusing_send_guard_exits_5_and_nothing_reaches_the_provider() {
    let tmp = Temp::new("guard");
    let root = setup(&tmp.0, false);
    let p = provider();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let args: Vec<String> = [
        "verify-consumers",
        root.to_str().unwrap(),
        "--base-url",
        &p.base,
    ]
    .map(String::from)
    .into();
    let code = rt_cli::run_distribution(
        args,
        &mut o,
        &mut e,
        &rt_cli::Distribution {
            guard: GuardHandle::new(Arc::new(Refuse)),
            ..Default::default()
        },
    );
    assert_eq!(
        code,
        rt_cli::EXIT_BLOCKED,
        "{}",
        String::from_utf8_lossy(&o)
    );
    assert!(p.hits.lock().unwrap().is_empty());
    assert!(String::from_utf8_lossy(&o).contains("not today"));
}

#[test]
fn usage_errors_exit_2_with_a_reason() {
    let tmp = Temp::new("usage");
    let root = setup(&tmp.0, false);
    let p = provider();
    let o = rt(&tmp.0, &root, &p.base, &["--consumer", "nobody"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).contains("nobody") && err(&o).contains("billing"),
        "{}",
        err(&o)
    );
    let o = rt(&tmp.0, &root, &p.base, &["--out", "xml"]);
    assert_eq!(o.status.code(), Some(2));
    // A collection that lists nobody.
    let bare = tmp.0.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::write(
        bare.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: a\n",
    )
    .unwrap();
    let o = rt(&tmp.0, &bare, &p.base, &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("lists no consumers"), "{}", err(&o));
    let o = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(["verify-consumers", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("--base-url"));
}

#[test]
fn a_consumer_that_cannot_run_still_breaks_the_build() {
    let tmp = Temp::new("ghost");
    let root = setup(&tmp.0, false);
    std::fs::remove_dir_all(tmp.0.join("portal")).unwrap();
    let p = provider();
    let o = rt(&tmp.0, &root, &p.base, &[]);
    assert_eq!(o.status.code(), Some(8));
    assert!(
        out(&o).contains("Consumer portal could not be verified"),
        "{}",
        out(&o)
    );
}

/// `file://` URL of a local path that `git` accepts: `file:///C:/x` on Windows, `file:///x` elsewhere.
fn file_url(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    // `canonicalize` gives `\\?\C:\x` on Windows.
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_owned();
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}
