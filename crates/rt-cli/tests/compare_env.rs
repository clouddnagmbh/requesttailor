// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 18, strand A: `rt compare-env`. Two local servers stand for two environments; they
//! answer the same request with a different `name`.

#[path = "support/watch_server.rs"]
mod server;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SECRET: &str = "RT-CANARY-compare-env-5e1d";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-compare-env-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn collection(root: &Path, dev: u16, qa: u16) {
    write(
        &root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: Items\n",
    );
    for (env, port) in [("dev", dev), ("qa", qa), ("prod", qa)] {
        write(
            &root.join(format!("environments/{env}.yml")),
            &format!(
                "name: {env}\nvariables:\n  - name: base\n    value: http://127.0.0.1:{port}\n  - secret: true\n    name: token\n"
            ),
        );
    }
    write(
        &root.join("items/folder.yml"),
        "info:\n  name: items\n  type: folder\n  seq: 1\n",
    );
    write(
        &root.join("items/get.yml"),
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/item?token={{token}}\"\n",
    );
    write(
        &root.join("items/post.yml"),
        "info:\n  name: Post\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: \"{{base}}/item\"\n  body:\n    type: json\n    data: '{\"a\":1}'\n",
    );
    write(
        &root.join("other.yml"),
        "info:\n  name: Other\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/other\"\n",
    );
}

fn rt(dir: &Path, root: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("compare-env")
        .arg(root)
        .args(["--var", &format!("token={SECRET}")])
        .args(extra)
        .env("RT_DATA_DIR", dir.join("data"))
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn lists_differences_skips_writes_and_exits_zero() {
    let (dev, qa) = (server::spawn(), server::spawn());
    *qa.name.lock().unwrap() = "coffee".into();
    let dir = tmp("text");
    let root = dir.join("c");
    collection(&root, dev.port, qa.port);
    let out = rt(&dir, &root, &["--env", "dev", "--env", "qa"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("dev (reference) vs qa: 2 of 2 requests differ"), "{text}");
    assert!(text.contains("differs  items/get.yml: body /name"), "{text}");
    assert!(text.contains("skipped (writes)  items/post.yml"), "{text}");
    // Reads went to both servers, the write to neither.
    for server in [&dev, &qa] {
        let seen: Vec<String> = server.seen.lock().unwrap().iter().map(|(_, l)| l.clone()).collect();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen.iter().all(|l| l.starts_with("GET ")), "{seen:?}");
    }
    assert!(!text.contains(SECRET) && !stderr(&out).contains(SECRET));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn same_environments_are_same_and_fail_on_diff_decides_the_exit_code() {
    let (dev, qa) = (server::spawn(), server::spawn());
    let dir = tmp("same");
    let root = dir.join("c");
    collection(&root, dev.port, qa.port);
    let out = rt(&dir, &root, &["--env", "dev", "--env", "qa", "--fail-on-diff"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("0 of 2 requests differ"));
    *qa.name.lock().unwrap() = "coffee".into();
    let out = rt(&dir, &root, &["--env", "dev", "--env", "qa", "--fail-on-diff"]);
    assert_eq!(out.status.code(), Some(1));
    let out = rt(&dir, &root, &["--env", "dev", "--env", "qa"]);
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_output_carries_the_comparison_without_secrets() {
    let (dev, qa) = (server::spawn(), server::spawn());
    *qa.name.lock().unwrap() = "coffee".into();
    let dir = tmp("json");
    let root = dir.join("c");
    collection(&root, dev.port, qa.port);
    let out = rt(&dir, &root.join("items"), &["--env", "dev", "--env", "qa", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["reference"], "dev");
    assert_eq!(json["pairs"][0]["environment"], "qa");
    assert_eq!(json["pairs"][0]["requests"][0]["body"][0]["pointer"], "/name");
    assert_eq!(json["skipped"][0]["path"], "items/post.yml");
    // The folder `items` limits the run to the folder: `other.yml` was not sent.
    assert_eq!(json["pairs"][0]["requests"].as_array().unwrap().len(), 1);
    assert!(!stdout(&out).contains(SECRET));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_errors_exit_two_and_production_gets_a_banner() {
    let (dev, qa) = (server::spawn(), server::spawn());
    let dir = tmp("usage");
    let root = dir.join("c");
    collection(&root, dev.port, qa.port);
    let out = rt(&dir, &root, &["--env", "dev"]);
    assert_eq!(out.status.code(), Some(2));
    let out = rt(&dir, &root, &["--env", "dev", "--env", "nowhere"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("nowhere"));
    assert!(dev.seen.lock().unwrap().is_empty());
    let out = rt(&dir, &root, &["--env", "dev", "--env", "prod"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stderr(&out).contains("PRODUCTION"), "{}", stderr(&out));
    let _ = std::fs::remove_dir_all(&dir);
}
