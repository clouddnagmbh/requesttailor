// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt send --cookie-jar FILE` (wave 5, strand D): cookies set by a response are stored in
//! FILE per environment, sent on the next run, masked in the report; a file that is not a
//! jar is refused and left alone.

use std::path::{Path, PathBuf};
use std::process::Command;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rt-cli-cookies-{tag}-{}", std::process::id()));
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

fn collection(dir: &Path, base: &str) -> PathBuf {
    let root = dir.join("collection");
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: cookies\n",
    )
    .unwrap();
    for env in ["Local", "Prod"] {
        std::fs::write(
            root.join(format!("environments/{env}.yml")),
            format!("name: {env}\nvariables:\n  - name: base\n    value: {base}\n"),
        )
        .unwrap();
    }
    for (file, url) in [
        (
            "login.yml",
            "{{base}}/cookies/set?sid=jar-value-1&theme=dark",
        ),
        ("whoami.yml", "{{base}}/cookies"),
    ] {
        std::fs::write(
            root.join(file),
            format!(
                "info:\n  name: {file}\n  type: http\n\nhttp:\n  method: GET\n  url: \"{url}\"\n"
            ),
        )
        .unwrap();
    }
    root
}

/// Runs `rt send`; returns the exit code and the report.
fn send(root: &Path, request: &str, args: &[&str]) -> (i32, serde_json::Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("send")
        .arg(root.join(request))
        .args(args)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.code().unwrap_or(-1), report)
}

/// The `cookie` header `GET /cookies` saw.
fn echoed(report: &serde_json::Value) -> Option<String> {
    let body = report["response"]["body"]["data"].as_str().unwrap();
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    json["cookie"].as_str().map(str::to_owned)
}

fn header<'a>(report: &'a serde_json::Value, side: &str, name: &str) -> Vec<&'a str> {
    report[side]["headers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h[0].as_str().unwrap().eq_ignore_ascii_case(name))
        .map(|h| h[1].as_str().unwrap())
        .collect()
}

#[test]
fn cookie_jar_file_per_environment() {
    let server = tokio::runtime::Runtime::new().unwrap();
    let addr = server.block_on(rt_testserver::spawn_local());
    let tmp = TempDir::new("flow");
    let root = collection(&tmp.0, &format!("http://{addr}"));
    let jar = tmp.0.join("jar.json");
    let jar_arg = jar.to_str().unwrap();
    let local = ["--env", "Local", "--cookie-jar", jar_arg];

    // Without --cookie-jar nothing is kept.
    let (code, _) = send(&root, "login.yml", &["--env", "Local"]);
    assert_eq!(code, 0);
    assert!(!jar.exists());

    let (code, report) = send(&root, "login.yml", &local);
    assert_eq!(code, 0, "{report:#}");
    assert_eq!(
        header(&report, "response", "set-cookie"),
        ["sid=***; Path=/", "theme=***; Path=/"]
    );
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&jar).unwrap()).unwrap();
    assert_eq!(stored["version"], 1);
    assert_eq!(stored["scopes"]["Local"].as_array().unwrap().len(), 2);
    assert!(stored["scopes"].get("Prod").is_none());

    // Sent on the next run in Local, masked in its report; not in Prod.
    let (_, report) = send(&root, "whoami.yml", &local);
    assert_eq!(
        echoed(&report).as_deref(),
        Some("sid=jar-value-1; theme=dark")
    );
    assert_eq!(header(&report, "request", "cookie"), ["sid=***; theme=***"]);
    let (_, report) = send(
        &root,
        "whoami.yml",
        &["--env", "Prod", "--cookie-jar", jar_arg],
    );
    assert_eq!(echoed(&report), None);
    let (_, report) = send(&root, "whoami.yml", &["--env", "Local"]);
    assert_eq!(echoed(&report), None, "no jar without --cookie-jar");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&jar).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
}

#[test]
fn a_file_that_is_not_a_jar_is_refused() {
    let server = tokio::runtime::Runtime::new().unwrap();
    let addr = server.block_on(rt_testserver::spawn_local());
    let tmp = TempDir::new("refused");
    let root = collection(&tmp.0, &format!("http://{addr}"));
    let jar = tmp.0.join("notes.txt");
    std::fs::write(&jar, "my notes, not cookies\n").unwrap();
    let (code, report) = send(
        &root,
        "login.yml",
        &["--env", "Local", "--cookie-jar", jar.to_str().unwrap()],
    );
    assert_ne!(code, 0);
    assert!(report["response"].is_null(), "nothing was sent: {report:#}");
    let message = report["error"].to_string();
    assert!(
        message.contains("not a RequestTailor cookie jar"),
        "{message}"
    );
    assert_eq!(
        std::fs::read_to_string(&jar).unwrap(),
        "my notes, not cookies\n"
    );
}
