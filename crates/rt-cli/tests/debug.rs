// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand Y: `rt send --debug [--break N]`, the script debugger in the terminal.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SECRET: &str = "RT-CANARY-debug-secret-1";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-debug-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = vec![0u8; 65536];
            let _ = stream.read(&mut buf);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
        }
    });
    port
}

const SCRIPT: &str = "const who = 'debugger';\\n      const n = 6;\\n      rt.request.headers.set('X-Debugged', who + n);\\n      console.log('token', rt.vars.get('token'));\\n";

fn collection(root: &Path, port: u16) {
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Items\n",
    )
    .unwrap();
    std::fs::write(
        root.join("environments/Local.yml"),
        "name: Local\nvariables:\n  - secret: true\n    name: token\n",
    )
    .unwrap();
    std::fs::write(
        root.join("get.yml"),
        format!(
            "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://127.0.0.1:{port}/item\"\n\nruntime:\n  scripts:\n    - type: before-request\n      code: \"{SCRIPT}\"\n"
        ),
    )
    .unwrap();
}

fn rt(extra: &[&str], stdin: &str, data: &Path, file: &Path) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("send")
        .arg(file)
        .args([
            "-e",
            "Local",
            "--no-plugins",
            "--var",
            &format!("token={SECRET}"),
        ])
        .args(extra)
        .env("RT_DATA_DIR", data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_breakpoint_stops_shows_variables_and_answers_expressions() {
    let port = server();
    let dir = tmp("break");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, port);
    let (code, out, err) = rt(
        &["--break", "3"],
        "p n * 7\np rt.vars.get('token')\nn\nc\n",
        &data,
        &root.join("get.yml"),
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("stopped at line 3 (breakpoint)"), "{err}");
    assert!(err.contains("> "), "{err}");
    assert!(err.contains("who = \"debugger\""), "{err}");
    assert!(err.contains("= 42 (number)"), "{err}");
    // Stepping over line 3 reaches line 4.
    assert!(err.contains("stopped at line 4 (step)"), "{err}");
    assert!(err.contains("[debug] script finished"), "{err}");
    // The script did its work: the report shows the header it set.
    assert!(out.contains("X-Debugged"), "{out}");
    assert!(out.contains("debugger6"), "{out}");
    // The secret is masked in everything the debugger showed and in the report.
    assert!(!err.contains(SECRET), "the secret leaked on stderr: {err}");
    assert!(!out.contains(SECRET), "the secret leaked on stdout: {out}");
    assert!(err.contains("***"), "{err}");
}

#[test]
fn without_input_the_script_continues_and_without_break_it_stops_at_the_start() {
    let port = server();
    let dir = tmp("entry");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, port);
    let (code, out, err) = rt(&["--debug"], "", &data, &root.join("get.yml"));
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("stopped at line 1 (entry)"), "{err}");
    assert!(out.contains("debugger6"), "{out}");
    assert!(!err.contains(SECRET) && !out.contains(SECRET));
}

#[test]
fn quit_ends_the_session_and_the_send() {
    let port = server();
    let dir = tmp("quit");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, port);
    let (code, out, err) = rt(&["--break", "2"], "q\n", &data, &root.join("get.yml"));
    assert_ne!(code, 0, "{out}");
    assert!(err.contains("[debug] session ended"), "{err}");
    assert!(!out.contains("debugger6"), "{out}");
}

#[test]
fn a_script_run_without_debug_is_unchanged() {
    let port = server();
    let dir = tmp("plain");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, port);
    let (code, out, err) = rt(&[], "", &data, &root.join("get.yml"));
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("[debug]"), "{err}");
    assert!(out.contains("debugger6"), "{out}");
}

#[test]
fn a_break_line_must_be_a_line() {
    let dir = tmp("zero");
    let (code, _, err) = rt(&["--break", "0"], "", &dir, &dir.join("x.yml"));
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("line number"), "{err}");
}
