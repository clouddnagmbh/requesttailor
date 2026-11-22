// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt scan` (wave 5, strand E) on the built binary: exit codes, the text and JSON output,
//! suppression, folders. Fake credentials are assembled at run time (gitleaks stays clean).

use std::path::{Path, PathBuf};
use std::process::Command;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str, files: &[(&str, &str)]) -> Dir {
    let dir = std::env::temp_dir().join(format!("rt-scan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rt_scan(cwd: &Path, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("scan")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run rt");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn token() -> String {
    ["9c1e2f3a4b5c", "6d7e8f90a1b2"].concat()
}

fn request_with_token() -> String {
    format!(
        "info:\n  name: Me\n  type: http\n\nhttp:\n  method: GET\n  url: https://api.example.com/me\n  auth:\n    type: bearer\n    token: {}\n",
        token()
    )
}

const ROOT: &str = "opencollection: \"1.0.0\"\ninfo:\n  name: api\n";
const CLEAN: &str = "info:\n  name: Me\n  type: http\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/me\"\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n";

#[test]
fn a_clean_collection_exits_0() {
    let d = temp(
        "clean",
        &[("opencollection.yml", ROOT), ("users/me.yml", CLEAN)],
    );
    let out = rt_scan(&d.0, &[]);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.stdout, "No credentials found (2 files scanned).\n");
}

#[test]
fn a_plaintext_token_exits_1_with_a_masked_finding_and_a_hint() {
    let d = temp(
        "found",
        &[
            ("opencollection.yml", ROOT),
            ("users/me.yml", &request_with_token()),
            ("users/clean.yml", CLEAN),
        ],
    );
    let out = rt_scan(&d.0, &[]);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout
            .starts_with("users/me.yml:10: token\n    token: ***\n    Make it a secret variable"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("{{token}}"), "{}", out.stdout);
    assert!(
        out.stdout
            .ends_with("1 credential in 1 file (3 files scanned). Move them into secret variables, or mark a line that is not a credential with `# rt-scan:allow`.\n"),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains(&token()));
    assert!(!out.stderr.contains(&token()));
}

#[test]
fn json_output() {
    let d = temp(
        "json",
        &[("a.yml", &request_with_token()), ("b.yml", CLEAN)],
    );
    let out = rt_scan(&d.0, &["--json", "a.yml", "b.yml"]);
    assert_eq!(out.code, 1, "{}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["scanned"], 2);
    let f = &v["findings"][0];
    assert_eq!(f["path"], "a.yml");
    assert_eq!(f["line"], 10);
    assert_eq!(f["kind"], "token");
    assert_eq!(f["snippet"], "token: ***");
    assert!(f["hint"].as_str().unwrap().contains("secret"));
    assert_eq!(v["findings"].as_array().unwrap().len(), 1);
    assert!(!out.stdout.contains(&token()));
    // Clean files give an empty list and exit 0.
    let out = rt_scan(&d.0, &["--json", "b.yml"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout.trim(), r#"{"findings":[],"scanned":1}"#);
}

#[test]
fn a_marked_line_is_not_a_finding() {
    let text = request_with_token().replace(&token(), &format!("{} # rt-scan:allow", token()));
    let d = temp("allow", &[("a.yml", &text)]);
    assert_eq!(rt_scan(&d.0, &[]).code, 0);
}

#[test]
fn folders_skip_git_and_binaries_and_missing_paths_exit_2() {
    let d = temp(
        "folders",
        &[
            ("col/opencollection.yml", ROOT),
            (".git/config", &request_with_token()),
            ("node_modules/x/a.yml", &request_with_token()),
        ],
    );
    std::fs::write(d.0.join("col/logo.png"), [0x89, b'P', b'N', b'G', 0, 1, 2]).unwrap();
    let out = rt_scan(&d.0, &["col"]);
    assert_eq!(out.code, 0, "{}", out.stdout);
    assert_eq!(out.stdout, "No credentials found (1 file scanned).\n");
    let out = rt_scan(&d.0, &[]);
    assert_eq!(out.code, 0, "{}", out.stdout);
    let out = rt_scan(&d.0, &["nope.yml"]);
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("cannot read nope.yml"),
        "{}",
        out.stderr
    );
    let out = rt_scan(&d.0, &["--bogus"]);
    assert_eq!(out.code, 2);
}

#[test]
fn help_documents_scan() {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("rt scan [<path>...] [--json]"), "{help}");
    assert!(help.contains("rt-scan:allow"), "{help}");
    assert!(help.contains("--allow-secrets"), "{help}");
}
