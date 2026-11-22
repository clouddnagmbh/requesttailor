// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt diff` (wave 12, strand A) on the built binary, against a real repository made with the
//! `git` command line: the working state against HEAD, one commit, two revisions, `--json`.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(dir: &Path, path: &str, text: &str) {
    let p = dir.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn request(url: &str) -> String {
    format!(
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: {url}\n  auth: inherit\n"
    )
}

fn repo(name: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("rt-diff-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    write(
        &dir,
        "opencollection.yml",
        "opencollection: 1.0.0\n\ninfo:\n  name: demo\n",
    );
    write(&dir, "get.yml", &request("http://x/a"));
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    write(&dir, "get.yml", &request("http://x/b"));
    git(&dir, &["commit", "-q", "-am", "two"]);
    Dir(dir)
}

fn rt(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("diff")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("rt");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn working_state_against_head() {
    let d = repo("working");
    assert_eq!(rt(&d.0, &[]).1, "No changes.\n");
    write(&d.0, "get.yml", &request("http://x/c"));
    let (code, out, _) = rt(&d.0, &[]);
    assert_eq!(code, 0);
    assert!(
        out.contains("~ request \"Get\" modified  [get.yml]"),
        "{out}"
    );
    assert!(
        out.contains("~ url: \"http://x/b\" -> \"http://x/c\"  (line 8)"),
        "{out}"
    );
}

#[test]
fn a_commit_against_its_parent_and_two_revisions() {
    let d = repo("commit");
    let (_, out, _) = rt(&d.0, &["HEAD"]);
    assert!(out.contains("\"http://x/a\" -> \"http://x/b\""), "{out}");
    let (_, out, _) = rt(&d.0, &["HEAD~1..HEAD"]);
    assert!(out.contains("\"http://x/a\" -> \"http://x/b\""), "{out}");
    // The root commit against nothing: everything added.
    let (_, out, _) = rt(&d.0, &["HEAD~1"]);
    assert!(out.contains("+ request \"Get\" added"), "{out}");
    assert!(out.contains("+ collection \"demo\" added"), "{out}");
}

#[test]
fn json_and_errors() {
    let d = repo("json");
    let (code, out, _) = rt(&d.0, &["--json", "HEAD"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["changes"][0]["fields"][0]["path"][0], "url");
    assert_eq!(v["files"][0]["covered"], true);
    let (code, _, err) = rt(&d.0, &["nope"]);
    assert_eq!(code, 2);
    assert!(err.contains("rt diff:"), "{err}");
}
