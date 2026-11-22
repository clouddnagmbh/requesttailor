// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Runs the built `rt` binary: exit codes and the JSON report, including an unchanged Bruno
//! collection sent through the CLI.
//!
//! Run all: `cargo test -p rt-cli --test cli -- --include-ignored`

#[path = "../../../testdata/conformance/harness.rs"]
mod harness;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use harness::Target;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs `rt` with a deadline, so a hanging binary fails the test instead of blocking it.
fn rt(args: &[String]) -> Output {
    // A data folder of its own: the developer's installed plugins must not change the reports.
    let data = std::env::temp_dir().join(format!("rt-cli-test-data-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_rt"))
        .args(args)
        .env("RT_DATA_DIR", data)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start rt");
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let (mut o, mut e) = (String::new(), String::new());
        let _ = out.read_to_string(&mut o);
        let _ = err.read_to_string(&mut e);
        (o, e)
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
    let (stdout, stderr) = reader.join().unwrap();
    Output {
        code: status.code().unwrap_or(-1),
        stdout,
        stderr,
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn version_and_help() {
    let v = rt(&strings(&["--version"]));
    assert_eq!(v.code, 0);
    assert_eq!(v.stdout.trim(), format!("rt {}", env!("CARGO_PKG_VERSION")));
    let h = rt(&strings(&["--help"]));
    assert_eq!(h.code, 0);
    assert!(h.stdout.contains("Exit codes:"), "{}", h.stdout);
    assert!(h.stdout.contains("rt send <request-file>"), "{}", h.stdout);
}

#[test]
fn usage_errors_exit_2() {
    for args in [
        strings(&[]),
        strings(&["send"]),
        strings(&["send", "--bogus", "x.yml"]),
        strings(&["send", "does/not/exist.yml"]),
        // A file outside any collection.
        vec![
            "send".to_owned(),
            testdata()
                .join("collections/bruno/README.md")
                .display()
                .to_string(),
        ],
    ] {
        let o = rt(&args);
        assert_eq!(o.code, 2, "{args:?}: {}", o.stderr);
        assert!(o.stdout.is_empty(), "{args:?}: {}", o.stdout);
        assert!(o.stderr.starts_with("rt: "), "{args:?}: {}", o.stderr);
    }
}

/// `rt send` for one conformance case, checked with the shared harness.
fn run_case(request: &str, collection: &str, port: u16, expected_code: i32, pretty: bool) {
    let suite = harness::suite();
    let case = suite
        .cases
        .iter()
        .find(|c| c.request == request && c.collection == collection && c.vars.is_empty())
        .unwrap_or_else(|| panic!("no case {collection}/{request}"));
    let plan = suite.plan(case, port);
    let mut args = vec![
        "send".to_owned(),
        testdata()
            .join(&plan.collection_path)
            .join(&plan.request)
            .display()
            .to_string(),
    ];
    if let Some(env) = &plan.environment {
        args.extend(["--env".to_owned(), env.clone()]);
    }
    for (name, value) in &plan.vars {
        args.extend(["--var".to_owned(), format!("{name}={value}")]);
    }
    if pretty {
        args.push("--pretty".to_owned());
    }
    let o = rt(&args);
    assert_eq!(
        o.code, expected_code,
        "{args:?}\nstdout: {}\nstderr: {}",
        o.stdout, o.stderr
    );
    let report: serde_json::Value = serde_json::from_str(&o.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", o.stdout));
    assert_eq!(o.stdout.contains("\n  \""), pretty, "{}", o.stdout);
    let failures = harness::check(case, Target::Native, port, &report);
    assert!(
        failures.is_empty(),
        "{}: {failures:#?}\n{report:#}",
        case.name
    );
}

#[test]
fn sends_cases_through_the_binary() {
    let server = tokio::runtime::Runtime::new().unwrap();
    let port = server.block_on(rt_testserver::spawn_local()).port();
    // Acceptance: an unchanged Bruno collection, base URL overridden with --var.
    run_case("GET-Request.yml", "bruno-methods", port, 0, false);
    run_case("POST-Request.yml", "bruno-methods", port, 0, true);
    run_case("basics/get.yml", "conformance", port, 0, false);
    run_case("basics/status.yml", "conformance", port, 0, false);
    run_case("basics/bad-host.yml", "conformance", port, 3, false);
    run_case("transport/refused.yml", "conformance", port, 1, false);
}
