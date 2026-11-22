// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt bisect` (wave 12, strand C): arguments and end-to-end searches of the built binary in a
//! real Git repository of 50 commits against `rt-testserver`. From commit 37 on, an assertion
//! of one request expects `200` but the request goes to `/status/500`.

use std::path::{Path, PathBuf};
use std::process::Command;

use rt_cli::{Command as Cli, EXIT_OK, EXIT_TESTS, EXIT_USAGE, parse_args};

// ---------------------------------------------------------------------------------------
// Arguments

#[test]
fn parses_the_options() {
    let Ok(Cli::Bisect(a)) = parse_args([
        "bisect", "--good", "v1", "--bad", "main", "-e", "Local", "--var", "a=1", "api",
    ]) else {
        panic!("not parsed")
    };
    assert_eq!(a.good, "v1");
    assert_eq!(a.bad, "main");
    assert_eq!(a.environment.as_deref(), Some("Local"));
    assert_eq!(a.vars, [("a".to_owned(), "1".to_owned())]);
    assert_eq!(a.path.as_deref(), Some(Path::new("api")));
}

#[test]
fn good_and_bad_are_required() {
    for args in [
        vec!["bisect"],
        vec!["bisect", "--good", "a"],
        vec!["bisect", "--bad", "b"],
    ] {
        let err = parse_args(args.clone()).unwrap_err();
        assert!(
            err.contains("--good") || err.contains("--bad"),
            "{args:?}: {err}"
        );
    }
    assert!(matches!(parse_args(["bisect", "--help"]), Ok(Cli::Help)));
}

// ---------------------------------------------------------------------------------------
// End to end

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rt-cli-bisect-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git is installed");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn request(name: &str, seq: u32, url: &str, assertion: bool) -> String {
    let runtime = if assertion {
        "\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n"
    } else {
        ""
    };
    format!(
        "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{url}\"\n{runtime}"
    )
}

/// 50 commits in `<tmp>/col`; from commit 37 on `check.yml` goes to `/status/500`. Returns
/// the collection folder and the ids of commits 1 and 37.
fn repo(tmp: &Path, base: &str) -> (PathBuf, String, String) {
    let root = tmp.join("col");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    write(
        &root,
        "opencollection.yml",
        "opencollection: \"1.0.0\"\ninfo:\n  name: bisect\n",
    );
    write(
        &root,
        "environments/Local.yml",
        &format!("name: Local\nvariables:\n  - name: base\n    value: {base}\n"),
    );
    write(
        &root,
        "ping.yml",
        &request("Ping", 1, "{{base}}/echo/ping", false),
    );
    let (mut first, mut culprit) = (String::new(), String::new());
    for i in 1..=50 {
        let status = if i >= 37 { 500 } else { 200 };
        write(
            &root,
            "check.yml",
            &request("Check", 2, &format!("{{{{base}}}}/status/{status}"), true),
        );
        write(&root, "notes.txt", &format!("{i}\n"));
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", &format!("commit {i}")]);
        match i {
            1 => first = git(&root, &["rev-parse", "HEAD"]),
            37 => culprit = git(&root, &["rev-parse", "HEAD"]),
            _ => {}
        }
    }
    (root, first, culprit)
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rt(cwd: &Path, tmp: &Path, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("bisect")
        .args(args)
        .current_dir(cwd)
        .env("RT_DATA_DIR", tmp.join("data"))
        .output()
        .unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn server() -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(rt_testserver::spawn_local());
    (rt, format!("http://{addr}"))
}

/// Tested commits in the progress lines on stderr (`tested <short id> good|bad|skip`).
fn tested(stderr: &str) -> usize {
    stderr.lines().filter(|l| l.starts_with("tested ")).count()
}

#[test]
fn finds_commit_37_in_at_most_seven_runs() {
    let (_server, base) = server();
    let tmp = TempDir::new("find");
    let (root, first, culprit) = repo(&tmp.0, &base);
    let r = rt(
        &root,
        &tmp.0,
        &["--good", &first, "--bad", "HEAD", "-e", "Local"],
    );
    assert_eq!(r.code, EXIT_OK, "{}", r.stderr);
    assert!(r.stdout.contains(&culprit), "{}", r.stdout);
    assert!(r.stdout.contains("commit 37"), "{}", r.stdout);
    // What failed is named.
    assert!(r.stdout.contains("Check"), "{}", r.stdout);
    let runs = tested(&r.stderr);
    assert!((1..=7).contains(&runs), "{runs} runs:\n{}", r.stderr);
    // A tested commit's id shows in the progress.
    assert!(r.stderr.contains(" bad"), "{}", r.stderr);
}

#[test]
fn json_output_and_a_folder_argument() {
    let (_server, base) = server();
    let tmp = TempDir::new("json");
    let (root, first, culprit) = repo(&tmp.0, &base);
    let r = rt(
        &root.join("."),
        &tmp.0,
        &["--good", &first, "--bad", "main", "-e", "Local", "--json"],
    );
    assert_eq!(r.code, EXIT_OK, "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["first_bad"]["oid"], culprit.as_str());
    assert_eq!(v["candidates"], 49);
    assert!(v["steps"].as_array().unwrap().len() <= 7);
}

#[test]
fn nothing_to_find_exits_four() {
    let (_server, base) = server();
    let tmp = TempDir::new("none");
    let (root, first, _) = repo(&tmp.0, &base);
    // `HEAD~14` is commit 36: everything passes.
    let r = rt(
        &root,
        &tmp.0,
        &["--good", &first, "--bad", "HEAD~14", "-e", "Local"],
    );
    assert_eq!(r.code, EXIT_TESTS, "{}", r.stderr);
    assert!(
        r.stdout.is_empty() || r.stdout.contains("no"),
        "{}",
        r.stdout
    );
    assert!(r.stderr.contains("passes"), "{}", r.stderr);
    assert_eq!(tested(&r.stderr), 1);
}

#[test]
fn usage_errors() {
    let (_server, base) = server();
    let tmp = TempDir::new("usage");
    let (root, first, _) = repo(&tmp.0, &base);
    // Not in a collection.
    let r = rt(&tmp.0, &tmp.0, &["--good", "a", "--bad", "b"]);
    assert_eq!(r.code, EXIT_USAGE, "{}", r.stderr);
    // Unknown revision.
    let r = rt(&root, &tmp.0, &["--good", "nope", "--bad", "HEAD"]);
    assert_eq!(r.code, EXIT_USAGE, "{}", r.stderr);
    assert!(r.stderr.contains("nope"), "{}", r.stderr);
    // Unknown environment.
    let r = rt(
        &root,
        &tmp.0,
        &["--good", &first, "--bad", "HEAD", "-e", "Missing"],
    );
    assert_eq!(r.code, EXIT_USAGE, "{}", r.stderr);
    assert!(r.stderr.contains("Missing"), "{}", r.stderr);
    // Not a folder of the collection.
    let r = rt(
        &root,
        &tmp.0,
        &["--good", &first, "--bad", "HEAD", "nowhere"],
    );
    assert_eq!(r.code, EXIT_USAGE, "{}", r.stderr);
}
