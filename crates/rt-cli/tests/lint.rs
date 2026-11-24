// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt lint` (wave 18, strand F) on the built binary: exit code 7 at the thresholds, the text,
//! JSON and GitHub output, `--rule`, the collection's own settings, a folder inside a
//! collection. Reads files and sends nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

const CANARY: &str = "RT-CANARY-lint-cli-3";

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/lint")
        .join(name)
}

fn rt_lint(cwd: &Path, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("lint")
        .args(args)
        .current_dir(cwd)
        // Nothing may leave the machine; a stray proxy would show as a failure elsewhere.
        .env("RT_DATA_DIR", std::env::temp_dir().join("rt-lint-data"))
        .output()
        .expect("run rt");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn temp_copy(name: &str, tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("rt-lint-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(&fixture(name), &dir);
    Dir(dir)
}

#[test]
fn a_clean_collection_exits_0_and_says_100() {
    let out = rt_lint(&fixture("clean"), &["--fail-on", "warn", "--min-score", "100"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("health score 100/100"), "{}", out.stdout);
    assert!(out.stdout.contains("0 findings"), "{}", out.stdout);
}

#[test]
fn without_a_threshold_findings_do_not_fail() {
    let out = rt_lint(&fixture("shop"), &[]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    // The hard-coded host, the missing test, the duplicate name, the unused variables.
    for needle in [
        "lint/hard-coded-host",
        "lint/no-tests",
        "lint/duplicate-name",
        "lint/orphan-variable",
        "users/create.yml",
        "`legacyUrl`",
        "`apiKey`",
    ] {
        assert!(out.stdout.contains(needle), "{needle}: {}", out.stdout);
    }
}

#[test]
fn fail_on_gives_exit_7() {
    // The duplicate name is an error by default.
    let out = rt_lint(&fixture("shop"), &["--fail-on", "error"]);
    assert_eq!(out.code, 7, "{}{}", out.stdout, out.stderr);
    let out = rt_lint(&fixture("shop"), &["--fail-on", "warn"]);
    assert_eq!(out.code, 7);
    // Turning the error rule off leaves only warnings and info.
    let out = rt_lint(
        &fixture("shop"),
        &["--fail-on", "error", "--rule", "lint/duplicate-name=off"],
    );
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let out = rt_lint(
        &fixture("shop"),
        &["--fail-on", "warn", "--rule", "duplicate-name=off"],
    );
    assert_eq!(out.code, 7);
}

#[test]
fn min_score_gives_exit_7_below_and_0_at_the_score() {
    let report = rt_lint(&fixture("shop"), &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(report.stdout.trim()).unwrap();
    let score = json["score"]["score"].as_u64().unwrap();
    assert!(score > 0 && score < 100, "{score}");
    let at = rt_lint(&fixture("shop"), &["--min-score", &score.to_string()]);
    assert_eq!(at.code, 0, "{}{}", at.stdout, at.stderr);
    let above = rt_lint(&fixture("shop"), &["--min-score", &(score + 1).to_string()]);
    assert_eq!(above.code, 7, "{}{}", above.stdout, above.stderr);
    assert!(above.stderr.contains("below the minimum"), "{}", above.stderr);
}

#[test]
fn json_output_is_the_report() {
    let out = rt_lint(&fixture("shop"), &["--format", "json"]);
    assert_eq!(out.code, 0);
    let json: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(json["collection"], "Lint demo shop");
    assert_eq!(json["score"]["rules"].as_array().unwrap().len(), 4);
    assert_eq!(json["folders"][0]["folder"], "");
    let findings = json["findings"].as_array().unwrap();
    assert!(findings.iter().any(|f| f["rule"] == "no-tests" && f["path"] == "users/create.yml"));
    assert!(findings.iter().all(|f| f["message"].is_string() && f["level"].is_string()));
    // Run twice, same bytes.
    assert_eq!(rt_lint(&fixture("shop"), &["--format", "json"]).stdout, out.stdout);
}

#[test]
fn github_output_is_workflow_commands_with_files_below_the_folder() {
    let out = rt_lint(&fixture("shop"), &["--format", "github"]);
    assert_eq!(out.code, 0);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert!(
        lines.iter().any(|l| l.starts_with("::warning file=users/create.yml,title=lint/no-tests::")),
        "{}",
        out.stdout
    );
    assert!(lines.iter().any(|l| l.starts_with("::error file=users/")), "{}", out.stdout);
    assert!(lines.iter().any(|l| l.starts_with("::notice ")), "{}", out.stdout);
    // From the parent folder the files are named below the folder as given.
    let parent = fixture("");
    let out = rt_lint(&parent, &["shop", "--format", "github"]);
    assert!(out.stdout.contains("file=shop/users/create.yml"), "{}", out.stdout);
}

#[test]
fn the_collections_own_settings_apply_and_flags_override_them() {
    let dir = temp_copy("shop", "settings");
    let file = dir.0.join("opencollection.yml");
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("extensions:\n  requesttailor:\n    lint:\n      rules:\n        lint/duplicate-name: off\n        lint/no-tests: error\n      ignore:\n        - lint/orphan-variable:*\n      min_score: 100\n");
    std::fs::write(&file, text).unwrap();
    let out = rt_lint(&dir.0, &["--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    let rules: Vec<&str> = json["findings"].as_array().unwrap().iter().map(|f| f["rule"].as_str().unwrap()).collect();
    assert!(!rules.contains(&"duplicate-name"), "{rules:?}");
    assert!(!rules.contains(&"orphan-variable"), "{rules:?}");
    assert!(json["ignored"].as_u64().unwrap() >= 2);
    let no_tests = json["findings"].as_array().unwrap().iter().find(|f| f["rule"] == "no-tests").unwrap();
    assert_eq!(no_tests["level"], "error");
    // `min_score: 100` from the file fails the run.
    assert_eq!(out.code, 7, "{}", out.stderr);
    // The flag wins over the file.
    let out = rt_lint(&dir.0, &["--min-score", "0"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    // `--rule` wins over the file's levels.
    let out = rt_lint(&dir.0, &["--min-score", "0", "--rule", "no-tests=off", "--format", "json"]);
    let json: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert!(json["findings"].as_array().unwrap().iter().all(|f| f["rule"] != "no-tests"));
    // The files were not touched.
    assert!(std::fs::read_to_string(&file).unwrap().contains("min_score: 100"));
}

#[test]
fn a_folder_inside_a_collection_reports_that_folder() {
    let users = fixture("shop").join("users");
    let out = rt_lint(&users, &["--format", "json"]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let json: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert!(!findings.is_empty());
    assert!(findings.iter().all(|f| f["path"].as_str().unwrap().starts_with("users/")), "{findings:?}");
    let whole = rt_lint(&fixture("shop"), &["--format", "json"]);
    let whole: serde_json::Value = serde_json::from_str(whole.stdout.trim()).unwrap();
    let users_score = whole["folders"].as_array().unwrap().iter().find(|f| f["folder"] == "users").unwrap();
    assert_eq!(json["score"], users_score["score"]);
}

#[test]
fn usage_errors_exit_2() {
    let nothing = std::env::temp_dir().join(format!("rt-lint-none-{}", std::process::id()));
    std::fs::create_dir_all(&nothing).unwrap();
    let _guard = Dir(nothing.clone());
    let out = rt_lint(&nothing, &[]);
    assert_eq!(out.code, 2, "{}{}", out.stdout, out.stderr);
    assert!(out.stderr.contains("opencollection.yml"), "{}", out.stderr);
    assert_eq!(rt_lint(&fixture("shop"), &["--format", "xml"]).code, 2);
    assert_eq!(rt_lint(&fixture("shop"), &["--rule", "nope=warn"]).code, 2);
    assert_eq!(rt_lint(&fixture("shop"), &["--rule", "no-tests=loud"]).code, 2);
    assert_eq!(rt_lint(&fixture("shop"), &["--fail-on", "info"]).code, 2);
    assert_eq!(rt_lint(&fixture("shop"), &["--min-score", "high"]).code, 2);
    assert_eq!(rt_lint(&nothing, &["missing"]).code, 2);
}

#[test]
fn no_secret_value_reaches_any_output() {
    let dir = temp_copy("shop", "canary");
    // A secret in a URL and in an environment value; neither may be printed.
    std::fs::write(
        dir.0.join("users/leak.yml"),
        format!("info:\n  name: Leak\n  type: http\n  seq: 9\n\nhttp:\n  method: GET\n  url: \"https://u:{CANARY}@leak.example.test/x?token={CANARY}\"\n  auth: inherit\n"),
    )
    .unwrap();
    std::fs::write(
        dir.0.join("environments/Dev.yml"),
        format!("name: Dev\nvariables:\n  - name: stage\n    value: {CANARY}\n  - secret: true\n    name: apiKey\n"),
    )
    .unwrap();
    for format in ["text", "json", "github"] {
        let out = rt_lint(&dir.0, &["--format", format]);
        assert!(!out.stdout.contains(CANARY), "{format}: {}", out.stdout);
        assert!(!out.stderr.contains(CANARY), "{format}: {}", out.stderr);
        assert!(out.stdout.contains("leak.example.test"), "{format}");
    }
}

#[test]
fn help_names_the_command_and_exit_code() {
    let out = Command::new(env!("CARGO_BIN_EXE_rt")).args(["lint", "--help"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("rt lint"), "{text}");
    assert!(text.contains("7"), "{text}");
}
