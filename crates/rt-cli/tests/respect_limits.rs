// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand D: `rt run --respect-limits [--max-wait T]` against a local server that
//! states rate limits. Real time, so each case keeps its waits to about a second.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SECRET: &str = "RT-CANARY-limits-secret-1";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-limits-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

type Seen = Arc<Mutex<Vec<(String, String, Instant)>>>;

/// `/limited`: 200 with nothing left until reset in `reset` seconds; `/flaky`: 429 with
/// `Retry-After: 1` once, then 200; `/always429`: 429 with `Retry-After: 1` every time;
/// anything else: 200. Records method, path and arrival.
fn server(reset: u64) -> (u16, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Seen = Arc::default();
    let s = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = vec![0u8; 65536];
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
            let earlier = s
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, p, _)| *p == path)
                .count();
            s.lock()
                .unwrap()
                .push((method, path.clone(), Instant::now()));
            let answer = match path.as_str() {
                "/limited" => format!(
                    "HTTP/1.1 200 OK\r\nRateLimit-Limit: 1\r\nRateLimit-Remaining: 0\r\nRateLimit-Reset: {reset}\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok"
                ),
                "/flaky" if earlier == 0 => "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
                "/always429" => "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
                _ => "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok".to_owned(),
            };
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    (port, seen)
}

fn request(root: &Path, file: &str, seq: u32, method: &str, port: u16, path: &str) {
    std::fs::write(
        root.join(file),
        format!(
            "info:\n  name: {file}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {method}\n  url: \"http://127.0.0.1:{port}{path}?token={{{{token}}}}\"\n"
        ),
    )
    .unwrap();
}

/// A collection whose requests run in file order a, b, c.
fn collection(root: &Path, port: u16, paths: &[(&str, &str)]) {
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Limits\n",
    )
    .unwrap();
    std::fs::write(
        root.join("environments/Local.yml"),
        "name: Local\nvariables:\n  - secret: true\n    name: token\n",
    )
    .unwrap();
    for (i, (method, path)) in paths.iter().enumerate() {
        let name = format!("{}.yml", (b'a' + i as u8) as char);
        request(root, &name, i as u32 + 1, method, port, path);
    }
}

fn rt(root: &Path, extra: &[&str], data: &Path) -> (i32, String, Duration) {
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("run")
        .arg(root)
        .args(extra)
        .args([
            "--no-plugins",
            "--env",
            "Local",
            "--var",
            &format!("token={SECRET}"),
        ])
        .env("RT_DATA_DIR", data)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        started.elapsed(),
    )
}

fn gaps(seen: &Seen) -> Vec<Duration> {
    let seen = seen.lock().unwrap();
    seen.windows(2).map(|w| w[1].2 - w[0].2).collect()
}

#[test]
fn without_the_flag_nothing_waits() {
    let (port, seen) = server(30);
    let root = tmp("off");
    collection(&root, port, &[("GET", "/limited"), ("GET", "/other")]);
    let (code, stderr, took) = rt(&root, &[], &root.join("data"));
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains("waiting"), "{stderr}");
    assert!(took < Duration::from_secs(10));
    assert!(gaps(&seen)[0] < Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn it_waits_for_the_reset_and_says_so() {
    let (port, seen) = server(1);
    let root = tmp("wait");
    collection(&root, port, &[("GET", "/limited"), ("GET", "/other")]);
    let (code, stderr, _) = rt(&root, &["--respect-limits"], &root.join("data"));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("waiting before b.yml: rate limit: 0 left, resets in 1 s"),
        "{stderr}"
    );
    let gap = gaps(&seen)[0];
    assert!(
        gap >= Duration::from_millis(950) && gap < Duration::from_millis(2500),
        "{gap:?}"
    );
    assert!(!stderr.contains(SECRET), "{stderr}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_wait_beyond_max_wait_ends_the_run_with_the_reason() {
    let (port, seen) = server(30);
    let root = tmp("toolong");
    collection(
        &root,
        port,
        &[("GET", "/limited"), ("GET", "/other"), ("GET", "/third")],
    );
    let (code, stderr, took) = rt(
        &root,
        &["--respect-limits", "--max-wait", "2s"],
        &root.join("data"),
    );
    assert_eq!(code, 4, "{stderr}");
    assert!(
        stderr.contains("resets in 30 s") && stderr.contains("--max-wait"),
        "{stderr}"
    );
    assert_eq!(seen.lock().unwrap().len(), 1, "nothing after the stop");
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(!stderr.contains(SECRET), "{stderr}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_429_on_a_read_is_retried_after_the_wait() {
    let (port, seen) = server(1);
    let root = tmp("retry");
    collection(&root, port, &[("GET", "/flaky")]);
    let (code, stderr, _) = rt(&root, &["--respect-limits"], &root.join("data"));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("the server asks to retry after 1 s"),
        "{stderr}"
    );
    let seen_paths = seen.lock().unwrap().len();
    assert_eq!(seen_paths, 2);
    assert!(gaps(&seen)[0] >= Duration::from_millis(950));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_write_is_never_repeated() {
    let (port, seen) = server(1);
    let root = tmp("write");
    collection(&root, port, &[("POST", "/always429")]);
    let (code, stderr, took) = rt(&root, &["--respect-limits"], &root.join("data"));
    assert_eq!(code, 0, "a 429 alone does not fail a request: {stderr}");
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(!stderr.contains("waiting"), "{stderr}");
    assert!(took < Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&root);
}
