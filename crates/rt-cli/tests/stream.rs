// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send` on session requests (wave 8, strand A): WebSocket (selected message,
//! `--message`, `--duration`, `--until-close`) and HTTP with `--sse`, against rt-testserver.
//! The output is the masked `StreamLog`; exit codes as for a send.

use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SECRET: &str = "s3cr3t-ws-token";

fn server() -> SocketAddr {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tx.send(rt_testserver::spawn_local().await).unwrap();
            std::future::pending::<()>().await;
        });
    });
    rx.recv().unwrap()
}

/// A collection with a WebSocket request (two message variants, a secret in the URL, a
/// header and a message) and an HTTP request for SSE.
fn collection(addr: SocketAddr) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rt-cli-stream-{}-{}",
        std::process::id(),
        addr.port()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("environments")).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\n\ninfo:\n  name: streams\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("environments/Local.yml"),
        "name: Local\nvariables:\n  - secret: true\n    name: token\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("echo.yml"),
        format!(
            "info:\n  name: echo\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: ws://{addr}/ws/echo?t={{{{token}}}}\n  headers:\n    - name: X-Token\n      value: \"{{{{token}}}}\"\n  message:\n    - title: hello\n      selected: true\n      message:\n        type: text\n        data: hello {{{{token}}}}\n    - title: json\n      message:\n        type: json\n        data: '{{\"n\": 1}}'\n  auth:\n    type: none\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("close.yml"),
        format!(
            "info:\n  name: close\n  type: websocket\n  seq: 2\n\nwebsocket:\n  url: ws://{addr}/ws/echo?close=4002&after=2\n  message:\n    type: text\n    data: ping\n  auth:\n    type: none\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("events.yml"),
        format!(
            "info:\n  name: events\n  type: http\n  seq: 3\n\nhttp:\n  method: GET\n  url: http://{addr}/sse/count/3\n"
        ),
    )
    .unwrap();
    dir
}

struct Output {
    code: i32,
    json: serde_json::Value,
    stdout: String,
}

fn rt(dir: &Path, args: &[&str]) -> Output {
    let data = std::env::temp_dir().join(format!("rt-cli-stream-data-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(args)
        .current_dir(dir)
        .env("RT_DATA_DIR", data)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start rt");
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut o = String::new();
        let _ = out.read_to_string(&mut o);
        o
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("rt {args:?} did not finish within 30 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = reader.join().unwrap();
    let json = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    Output {
        code: status.code().unwrap_or(-1),
        json,
        stdout,
    }
}

fn events(json: &serde_json::Value) -> Vec<String> {
    json["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|e| match e["event"].as_str().unwrap() {
            "message" => format!(
                "{}:{}",
                e["direction"].as_str().unwrap(),
                e["frame"]["data"].as_str().unwrap()
            ),
            "closed" => format!("closed:{}", e["code"]),
            other => other.to_owned(),
        })
        .collect()
}

#[test]
fn websocket_sends_the_selected_message_and_stops_after_the_answer() {
    let addr = server();
    let dir = collection(addr);
    let var = format!("token={SECRET}");
    let out = rt(&dir, &["send", "echo.yml", "-e", "Local", "--var", &var]);
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert!(
        !out.stdout.contains(SECRET),
        "secret in the output: {}",
        out.stdout
    );
    assert_eq!(out.json["kind"], "websocket");
    assert_eq!(out.json["request_path"], "echo.yml");
    assert!(
        out.json["url"].as_str().unwrap().contains("t=***"),
        "{}",
        out.stdout
    );
    assert_eq!(
        events(&out.json),
        [
            "opened",
            "sent:hello ***",
            "received:hello ***",
            "closed:1000"
        ]
    );
}

#[test]
fn websocket_message_variant_duration_and_until_close() {
    let addr = server();
    let dir = collection(addr);
    let out = rt(
        &dir,
        &[
            "send",
            "echo.yml",
            "-e",
            "Local",
            "--message",
            "json",
            "--duration",
            "0.3",
        ],
    );
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert_eq!(
        events(&out.json),
        [
            "opened",
            "sent:{\"n\": 1}",
            "received:{\"n\": 1}",
            "closed:1000"
        ]
    );

    // The server closes after two echoes; with one message sent it waits until --duration.
    let started = Instant::now();
    let out = rt(
        &dir,
        &["send", "close.yml", "--until-close", "--duration", "1"],
    );
    // The later option wins: --duration 1.
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert!(started.elapsed() >= Duration::from_millis(900));
    let out = rt(&dir, &["send", "close.yml", "--duration", "0.2"]);
    assert_eq!(events(&out.json).last().unwrap(), "closed:1000");

    let out = rt(&dir, &["send", "echo.yml", "--message", "nope"]);
    assert_eq!(out.code, 2, "{}", out.stdout);
    assert_eq!(out.json["error"]["kind"], "not-found");
}

#[test]
fn sse_with_the_flag_until_the_stream_ends() {
    let addr = server();
    let dir = collection(addr);
    let out = rt(&dir, &["send", "events.yml", "--sse", "--until-close"]);
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert_eq!(out.json["kind"], "sse");
    assert_eq!(
        events(&out.json),
        [
            "opened",
            "received:1",
            "received:2",
            "received:3",
            "closed:null"
        ]
    );
    // Without --sse the same request is a plain send.
    let out = rt(&dir, &["send", "events.yml"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.json["response"]["status"], 200);
}

#[test]
fn a_session_that_cannot_open_exits_like_a_failed_send() {
    let addr = server();
    let dir = collection(addr);
    std::fs::write(
        dir.join("refused.yml"),
        "info:\n  name: refused\n  type: websocket\n\nwebsocket:\n  url: ws://127.0.0.1:9/\n  auth:\n    type: none\n",
    )
    .unwrap();
    let out = rt(&dir, &["send", "refused.yml"]);
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert_eq!(out.json["error"]["kind"], "transport");
}
