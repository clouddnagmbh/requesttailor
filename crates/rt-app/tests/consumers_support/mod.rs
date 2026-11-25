// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Shared by `consumers.rs` and `consumers_perf.rs` (wave 19, strand F): a tiny in-process
//! provider, the consumer fixtures and a driver that does what the shells do.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rt_app::consumers::{
    Budget, ConsumerReport, ConsumerVerifyRequest, Loaded, fetch, run_consumer,
};
use rt_core::send::SendOptions;
use rt_core::services::Services;
use rt_core::transport::native::NativeTransport;
use rt_format::{FileSet, read_collection};

pub const SECRET: &str = "RT-CANARY-consumers-secret-5d2a";
pub const PERSONAL: &str = "dora.canary@example.test";

pub struct Provider {
    pub base: String,
    /// `METHOD path` of every request, in order.
    pub hits: Arc<Mutex<Vec<String>>>,
    /// The deliberate change: `items[0].price` becomes a string.
    pub changed: Arc<AtomicBool>,
}

/// Answers the three consumers: `GET /orders/1`, `POST /orders`, `GET /orders`, `GET /health`.
/// With `changed` set the price is text (and holds the caller's key and a personal value).
pub fn provider() -> Provider {
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
                let line = head.lines().next().unwrap_or("").to_owned();
                let mut parts = line.split(' ');
                let method = parts.next().unwrap_or("").to_owned();
                let path = parts
                    .next()
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                h.lock().unwrap().push(format!("{method} {path}"));
                let key = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("x-api-key: ")
                            .map(str::to_owned)
                    })
                    .unwrap_or_default();
                let (status, body) = match (method.as_str(), path.as_str()) {
                    ("GET", "/orders/1") if c.load(Ordering::SeqCst) => (
                        200,
                        format!("{{\"id\":1,\"items\":[{{\"price\":\"{PERSONAL} {key}\"}}]}}"),
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

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/consumers")
}

pub struct Temp(pub PathBuf);

impl Temp {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rt-consumers-{name}-{}", std::process::id()));
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

pub fn copy_dir(from: &Path, to: &Path) {
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

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.test"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A Git repository with two commits holding the `mobile` consumer; returns its folder.
pub fn mobile_repo(tmp: &Path) -> PathBuf {
    let repo = tmp.join("mobile-repo");
    copy_dir(&fixtures().join("mobile"), &repo);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    std::fs::write(repo.join("README.md"), "mobile consumer\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "second"]);
    repo
}

/// A provider collection next to copies of the consumers, listing `entries` (YAML lines of the
/// `consumers` list); returns its folder.
pub fn provider_collection(tmp: &Path, entries: &str) -> PathBuf {
    let root = tmp.join("provider");
    std::fs::create_dir_all(&root).unwrap();
    for c in ["billing", "portal"] {
        copy_dir(&fixtures().join(c), &tmp.join(c));
    }
    std::fs::write(
        root.join("opencollection.yml"),
        format!(
            "opencollection: 1.0.0\ninfo:\n  name: orders provider\nextensions:\n  requesttailor:\n    consumers:\n{entries}"
        ),
    )
    .unwrap();
    root
}

pub const LOCAL_TWO: &str = "      - name: billing\n        source: ../billing\n        environment: ci\n      - name: portal\n        source: ../portal\n        environment: ci\n        vars: { baseUrl: \"{{provider.baseUrl}}\" }\n";

pub fn request(base: &str) -> ConsumerVerifyRequest {
    ConsumerVerifyRequest {
        base_url: base.to_owned(),
        only: Vec::new(),
        fetch: false,
        keep_going: true,
        trust_scripts: false,
        allow_writes: false,
        max_minutes: None,
    }
}

/// What a shell does: list, select, find, load (with the canary as the secret `apiKey`) and run.
pub fn verify(
    provider_root: &Path,
    data_dir: &Path,
    request: &ConsumerVerifyRequest,
) -> ConsumerReport {
    let files = FileSet::from_dir(provider_root).unwrap();
    let listed = rt_app::consumers::list(&files).unwrap();
    let chosen = rt_app::consumers::select(&listed, &request.only).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let transport = NativeTransport::new().unwrap();
    let options = SendOptions::default();
    let mut budget = Budget::new(request.max_minutes);
    let mut report = ConsumerReport::default();
    for consumer in chosen {
        let found = fetch::fetch(consumer, provider_root, Some(data_dir), request.fetch, None);
        let result = match found {
            Err(e) => rt_app::consumers::ConsumerResult {
                name: consumer.name.clone(),
                commit: None,
                requests: 0,
                skipped: 0,
                failures: Vec::new(),
                error: Some(e),
            },
            Ok(found) => {
                let files = FileSet::from_dir(&found.root).unwrap();
                let mut collection = read_collection(&files).unwrap().model().clone();
                for env in &mut collection.environments {
                    for v in env.variables.iter_mut().filter(|v| v.secret) {
                        v.value = Some(SECRET.to_owned());
                    }
                }
                let loaded = Loaded {
                    collection,
                    files,
                    commit: found.commit,
                };
                runtime.block_on(run_consumer(
                    consumer,
                    loaded,
                    request,
                    &mut budget,
                    &transport,
                    &options,
                    &Services::none(),
                    &|| false,
                ))
            }
        };
        report.consumers.push(result);
    }
    report
}
