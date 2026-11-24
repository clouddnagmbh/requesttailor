// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 17, strand D: `rt send --timing` (the one-line timing bar) and the production banner
//! of `rt send` and `rt run`: visible on stderr and in the report's notes, never a refusal.

#[path = "support/watch_server.rs"]
mod server;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;

const SECRET: &str = "RT-CANARY-send-banner-1";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-banner-{name}-{}", std::process::id()));
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
        "opencollection: \"1.0.0\"\ninfo:\n  name: Items\n\nextensions:\n  requesttailor:\n    agents:\n      access: read\n      environments:\n        Customer-A: { access: write, production: true }\n",
    );
    for env in ["Local", "PRD", "Customer-A"] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
            ),
        );
    }
    write(
        &root.join("items/get.yml"),
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/item?token={{token}}\"\n",
    );
    write(
        &root.join("items/post.yml"),
        "info:\n  name: Post\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{base}}/item?token={{token}}\"\n",
    );
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

#[test]
fn timing_prints_one_bar_and_one_sentence_on_stderr() {
    let server = server::spawn();
    server.delay_ms.store(60, Ordering::SeqCst);
    let dir = tmp("timing");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let get = root.join("items/get.yml");
    let (code, out, err) = rt(
        &["send", get.to_str().unwrap(), "-e", "Local", "--timing"],
        &data,
    );
    assert_eq!(code, 0, "{err}");
    // stdout stays the JSON report.
    serde_json::from_str::<serde_json::Value>(&out).unwrap();
    let line = err
        .lines()
        .find(|l| l.contains("Server wait is"))
        .unwrap_or_else(|| panic!("{err}"));
    assert!(line.contains('[') && line.contains(']'), "{line}");
    assert!(line.contains(" % of "), "{line}");
    assert_eq!(
        err.lines().filter(|l| l.contains("Server wait is")).count(),
        1
    );
    // Without the flag there is nothing.
    let (_, _, err) = rt(&["send", get.to_str().unwrap(), "-e", "Local"], &data);
    assert!(!err.contains("Server wait"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn production_shows_a_banner_and_a_note_but_never_refuses() {
    let server = server::spawn();
    let dir = tmp("banner");
    let (root, data) = (dir.join("c"), dir.join("data"));
    collection(&root, server.port);
    let (get, post) = (root.join("items/get.yml"), root.join("items/post.yml"));
    for env in ["PRD", "Customer-A"] {
        let (code, out, err) = rt(&["send", get.to_str().unwrap(), "-e", env], &data);
        assert_eq!(code, 0, "{env}: {err}");
        assert!(err.contains("PRODUCTION"), "{env}: {err}");
        assert!(err.contains(env), "{err}");
        assert!(!err.contains(SECRET), "{err}");
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        let notes = report["notes"]
            .as_array()
            .unwrap_or_else(|| panic!("{out}"));
        assert!(
            notes
                .iter()
                .any(|n| n["text"].as_str().is_some_and(|t| t.contains("PRODUCTION"))),
            "{out}"
        );
        // A writing request is named as such, and is still sent.
        let (code, _, err) = rt(&["send", post.to_str().unwrap(), "-e", env], &data);
        assert_eq!(code, 0, "{err}");
        assert!(err.contains("PRODUCTION") && err.contains("POST"), "{err}");
    }
    assert_eq!(server.seen.lock().unwrap().len(), 4);
    // Not production: no banner, no note.
    let (code, out, err) = rt(&["send", get.to_str().unwrap(), "-e", "Local"], &data);
    assert_eq!(code, 0);
    assert!(!err.contains("PRODUCTION"), "{err}");
    assert!(
        serde_json::from_str::<serde_json::Value>(&out)
            .unwrap()
            .get("notes")
            .is_none()
    );
    // `rt run` says it once, before the first request.
    let (code, _, err) = rt(&["run", root.to_str().unwrap(), "-e", "PRD"], &data);
    assert_eq!(code, 0, "{err}");
    assert_eq!(err.matches("PRODUCTION").count(), 1, "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
