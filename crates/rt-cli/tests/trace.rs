// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand A: `rt send --trace` and the opt-in `traceparent` of a collection.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

const SECRET: &str = "RT-CANARY-trace-secret-1";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-trace-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A server that records the head of every request and answers 200.
fn server() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = vec![0u8; 65536];
            let n = stream.read(&mut buf).unwrap_or(0);
            s.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
        }
    });
    (port, seen)
}

fn collection(root: &Path, port: u16, inject: bool) {
    let extra = if inject {
        "\nextensions:\n  requesttailor:\n    trace:\n      inject: true\n"
    } else {
        "\n"
    };
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        format!("opencollection: \"1.0.0\"\ninfo:\n  name: Items\n{extra}"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("environments/Local.yml"),
        "name: Local\nvariables:\n  - secret: true\n    name: token\n",
    )
    .unwrap();
    std::fs::write(
        root.join("get.yml"),
        format!("info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://127.0.0.1:{port}/item?token={{{{token}}}}\"\n"),
    )
    .unwrap();
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

fn traceparent(head: &str) -> Option<String> {
    head.lines()
        .find_map(|l| l.strip_prefix("traceparent: "))
        .map(str::to_owned)
}

#[test]
fn trace_prints_the_id_that_went_on_the_wire() {
    let (port, seen) = server();
    let dir = tmp("on");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, port, true);
    let file = root.join("get.yml");
    let (code, out, err) = rt(
        &["send", file.to_str().unwrap(), "-e", "Local", "--trace"],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    let id = report["trace"]["trace_id"].as_str().unwrap();
    assert_eq!(report["trace"]["injected"], true);
    assert!(err.contains(&format!("trace {id}")), "{err}");
    let head = seen.lock().unwrap()[0].to_lowercase();
    let sent = traceparent(&head).unwrap_or_else(|| panic!("{head}"));
    assert!(
        sent.starts_with(&format!("00-{id}-")) && sent.ends_with("-01"),
        "{sent}"
    );
    // The trace parts of the output carry no secret (the `response.url` of the report is a
    // separate matter, not of this strand).
    assert!(!err.contains(SECRET), "{err}");
    assert!(!report["trace"].to_string().contains(SECRET));
    assert!(!report["request"].to_string().contains(SECRET));
    // Without the flag nothing is printed on stderr.
    let (_, _, err) = rt(&["send", file.to_str().unwrap(), "-e", "Local"], &data);
    assert!(!err.contains("trace "), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_by_default_nothing_is_sent_and_the_flag_says_why() {
    let (port, seen) = server();
    let dir = tmp("off");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, port, false);
    let file = root.join("get.yml");
    let (code, out, err) = rt(
        &["send", file.to_str().unwrap(), "-e", "Local", "--trace"],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("no trace id"), "{err}");
    assert!(
        serde_json::from_str::<serde_json::Value>(&out)
            .unwrap()
            .get("trace")
            .is_none()
    );
    assert!(traceparent(&seen.lock().unwrap()[0].to_lowercase()).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
