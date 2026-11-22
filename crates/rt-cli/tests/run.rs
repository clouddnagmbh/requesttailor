// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt run` (wave 7, strand E): argument parsing, the reporters (golden files for JSON and
//! JUnit, built from synthetic steps) and end-to-end runs of the built binary against
//! `rt-testserver`.
//!
//! The end-to-end runs need the runner (strand D) and, with scripts, the sandbox (strand A);
//! until then they are ignored. Run all: `cargo test -p rt-cli --test run -- --include-ignored`.
//! Update the golden files: `RT_UPDATE_GOLDEN=1 cargo test -p rt-cli --test run`.

#[path = "support/xml.rs"]
mod xml;

use std::path::{Path, PathBuf};
use std::process::Command;

use rt_cli::run::{Reporter, ReporterKind, RunArgs, exit_code};
use rt_cli::{Command as Cli, EXIT_OK, EXIT_TESTS, EXIT_USAGE, parse_args, report};
use rt_core::runner::{RunStep, RunSummary};

// ---------------------------------------------------------------------------------------
// Arguments

fn run_args(args: &[&str]) -> RunArgs {
    match parse_args(std::iter::once("run").chain(args.iter().copied())) {
        Ok(Cli::Run(a)) => a,
        other => panic!("{args:?}: {other:?}"),
    }
}

fn run_err(args: &[&str]) -> String {
    match parse_args(std::iter::once("run").chain(args.iter().copied())) {
        Err(e) => e,
        other => panic!("{args:?} should fail: {other:?}"),
    }
}

#[test]
fn defaults() {
    let a = run_args(&[]);
    assert_eq!(a.path, None);
    assert_eq!(a.environment, None);
    assert!(a.vars.is_empty());
    assert_eq!(a.data, None);
    assert_eq!(a.iterations, None);
    assert_eq!(a.delay_ms, 0);
    assert!(!a.bail);
    assert!(a.reporters.is_empty(), "text on stderr is implied");
    assert!(a.prompt);
    assert!(!a.no_scripts && !a.no_plugins);
    assert_eq!(
        a.body_limit,
        rt_core::send::SendOptions::default().body_limit
    );
}

#[test]
fn parses_every_option() {
    let a = run_args(&[
        "api/users",
        "-e",
        "Local",
        "--var",
        "a=1",
        "--var=b=x=y",
        "--data",
        "rows.csv",
        "--iterations",
        "3",
        "--delay",
        "250",
        "--bail",
        "--reporter",
        "junit=out/report.xml",
        "--reporter",
        "json",
        "--out",
        "run.json",
        "--reporter",
        "text",
        "--no-scripts",
        "--plugin",
        "p.wasm",
        "--plugin",
        "q.wasm",
        "--no-plugins",
        "--no-prompt",
        "--cookie-jar",
        "jar.json",
        "--body-limit",
        "1000",
    ]);
    assert_eq!(a.path, Some(PathBuf::from("api/users")));
    assert_eq!(a.environment.as_deref(), Some("Local"));
    assert_eq!(
        a.vars,
        [
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "x=y".to_owned())
        ]
    );
    assert_eq!(a.data, Some(PathBuf::from("rows.csv")));
    assert_eq!(a.iterations, Some(3));
    assert_eq!(a.delay_ms, 250);
    assert!(a.bail);
    assert_eq!(
        a.reporters,
        [
            Reporter {
                kind: ReporterKind::Junit,
                out: Some("out/report.xml".into())
            },
            Reporter {
                kind: ReporterKind::Json,
                out: Some("run.json".into())
            },
            Reporter {
                kind: ReporterKind::Text,
                out: None
            },
        ]
    );
    assert!(a.no_scripts && a.no_plugins && !a.prompt);
    assert_eq!(
        a.plugins,
        [PathBuf::from("p.wasm"), PathBuf::from("q.wasm")]
    );
    assert_eq!(a.cookie_jar, Some(PathBuf::from("jar.json")));
    assert_eq!(a.body_limit, 1000);
    let short = run_args(&["--env", "E", "--reporter=junit"]);
    assert_eq!(short.environment.as_deref(), Some("E"));
    assert_eq!(short.reporters[0].out, None, "no file: stdout");
}

#[test]
fn rejects_bad_arguments() {
    for (args, needle) in [
        (&["a", "b"][..], "unexpected"),
        (&["--iterations", "0"], "--iterations"),
        (&["--iterations", "many"], "invalid"),
        (&["--delay", "-1"], "invalid"),
        (&["--reporter", "xml"], "reporter"),
        (&["--reporter", "junit="], "reporter"),
        (&["--out", "x.xml"], "--out"),
        (&["--reporter", "junit=a.xml", "--out", "b.xml"], "--out"),
        (&["--reporter", "json", "--reporter", "junit"], "stdout"),
        (
            &["--reporter", "json=r", "--reporter", "junit=r"],
            "same file",
        ),
        (&["--var", "novalue"], "NAME=VALUE"),
        (&["--var", "=v"], "NAME=VALUE"),
        (&["--frobnicate"], "invalid option"),
        (&["--env"], "missing"),
    ] {
        let e = run_err(args);
        assert!(e.contains(needle), "{args:?}: {e}");
    }
    assert_eq!(parse_args(["run", "--help"]), Ok(Cli::Help));
    assert_eq!(parse_args(["run", "-h"]), Ok(Cli::Help));
}

#[test]
fn help_documents_rt_run() {
    let h = rt_cli::help();
    for needle in [
        "rt run [<folder>]",
        "--reporter <NAME[=FILE]>",
        "--data <FILE>",
        "--iterations <N>",
        "--bail",
        "4  ",
    ] {
        assert!(h.contains(needle), "{needle}\n{h}");
    }
}

// ---------------------------------------------------------------------------------------
// Reporters, from synthetic steps

/// Two iterations of three requests: tests and assertions (one failing, with characters
/// that need escaping), a transport error, a skipped request. Secrets are already masked by
/// `send_with`; the reporters print what they get.
fn steps() -> Vec<RunStep> {
    let report = |path: &str, status: Option<u16>, extra: serde_json::Value| {
        let mut r = serde_json::json!({
            "version": 1,
            "request_path": path,
            "environment": "Local",
            "request": {
                "method": "GET",
                "url": format!("http://127.0.0.1:47865/{path}"),
                "headers": [["authorization", "Bearer ***"]],
                "body_size": 0
            },
            "response": status.map(|s| serde_json::json!({
                "status": s,
                "version": "HTTP/1.1",
                "headers": [["content-type", "application/json"]],
                "url": format!("http://127.0.0.1:47865/{path}"),
                "redirects": [],
                "body": { "encoding": "utf8", "data": "{\"ok\":true}" },
                "size": 11,
                "truncated": false,
                "timing": { "dns": null, "connect": 1.0, "tls": null, "ttfb": 10.0 },
                "total": 12.0
            })),
            "variables": [],
            "unresolved": [],
            "error": null
        });
        for (k, v) in extra.as_object().unwrap() {
            r[k] = v.clone();
        }
        r
    };
    let mut out = Vec::new();
    for iteration in 0..2u32 {
        let failing = iteration == 1;
        out.push(serde_json::json!({
            "iteration": iteration,
            "request_path": "users/get.yml",
            "request_name": "Get user",
            "passed": !failing,
            "skipped": false,
            "report": report("users/get.yml", Some(200), serde_json::json!({
                "tests": [
                    { "name": "status is 200", "passed": true, "origin": "request" },
                    {
                        "name": "name is <\"Ann\" & 'Bob'>",
                        "passed": !failing,
                        "error": if failing { serde_json::json!("expected 'Bo\u{1b}b' to equal 'Bob'\nat line 2") } else { serde_json::Value::Null },
                        "origin": "request"
                    }
                ],
                "assertions": [{
                    "expression": "res.status",
                    "operator": "eq",
                    "expected": "200",
                    "actual": "200",
                    "passed": true
                }],
                "logs": [{ "level": "log", "text": "token ***", "origin": "request" }]
            })),
        }));
        out.push(serde_json::json!({
            "iteration": iteration,
            "request_path": "users/create.yml",
            "request_name": "Create user",
            "passed": false,
            "skipped": false,
            "report": report("users/create.yml", None, serde_json::json!({
                "error": { "kind": "transport", "message": { "kind": "connect", "message": "connection refused" } }
            })),
        }));
        out.push(serde_json::json!({
            "iteration": iteration,
            "request_path": "cleanup.yml",
            "request_name": "Cleanup",
            "passed": true,
            "skipped": true,
            "report": report("cleanup.yml", None, serde_json::json!({ "request": null })),
        }));
    }
    out.into_iter()
        .map(|v| serde_json::from_value(v).expect("synthetic step"))
        .collect()
}

fn summary() -> RunSummary {
    RunSummary {
        requests: 6,
        passed: 3,
        failed: 3,
        skipped: 2,
        tests_passed: 3,
        tests_failed: 1,
        assertions_passed: 2,
        assertions_failed: 0,
        duration_ms: 1234,
        stopped: false,
    }
}

fn info() -> report::RunInfo {
    report::RunInfo {
        collection: "Users & Co".into(),
        iterations: 2,
    }
}

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("RT_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (RT_UPDATE_GOLDEN=1 creates it)", path.display()));
    assert_eq!(
        actual,
        expected.replace("\r\n", "\n"),
        "{name} changed; RT_UPDATE_GOLDEN=1 updates it"
    );
}

#[test]
fn json_report_golden() {
    let out = report::json(&summary(), &steps());
    golden("run.json", &out);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["version"], 1);
    assert_eq!(v["summary"]["failed"], 3);
    assert_eq!(v["steps"].as_array().unwrap().len(), 6);
    // The steps are the reports as `rt send` prints them.
    assert_eq!(
        v["steps"][0]["report"]["request"]["headers"][0][1],
        "Bearer ***"
    );
    let back: Vec<RunStep> = serde_json::from_value(v["steps"].clone()).unwrap();
    assert_eq!(back, steps());
}

#[test]
fn junit_report_golden() {
    let out = report::junit(&info(), &summary(), &steps());
    golden("run.junit.xml", &out);
}

#[test]
fn junit_report_is_valid_and_complete() {
    let out = report::junit(&info(), &summary(), &steps());
    let root = xml::parse(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
    assert_eq!(root.name, "testsuites");
    assert_eq!(root.attr("name"), "Users & Co");
    let suites = root.all("testsuite");
    let names: Vec<_> = suites.iter().map(|s| s.attr("name")).collect();
    assert_eq!(
        names,
        [
            "users/get.yml [iteration 1]",
            "users/create.yml [iteration 1]",
            "cleanup.yml [iteration 1]",
            "users/get.yml [iteration 2]",
            "users/create.yml [iteration 2]",
            "cleanup.yml [iteration 2]",
        ]
    );
    // One testcase per test and assertion.
    let get = suites[3];
    let cases: Vec<_> = get.all("testcase");
    assert_eq!(cases.len(), 3);
    assert_eq!(get.attr("tests"), "3");
    assert_eq!(get.attr("failures"), "1");
    assert_eq!(get.attr("errors"), "0");
    assert_eq!(get.attr("time"), "0.012");
    assert_eq!(cases[1].attr("name"), "name is <\"Ann\" & 'Bob'>");
    let failure = cases[1].child("failure").expect("failure");
    // Control characters cannot be in XML, not even as references: they are spelled out.
    assert_eq!(
        failure.attr("message"),
        "expected 'Bo\\u001bb' to equal 'Bob'\nat line 2"
    );
    assert_eq!(cases[2].attr("name"), "res.status eq 200");
    assert!(cases[0].child("failure").is_none());
    for c in &cases {
        assert_eq!(c.attr("classname"), "users/get.yml [iteration 2]");
    }
    // Script output goes to system-out.
    assert!(get.child("system-out").unwrap().text.contains("token ***"));
    // A request error is an <error>.
    let create = suites[1];
    assert_eq!(create.attr("errors"), "1");
    let error = create.all("error");
    assert_eq!(error.len(), 1);
    assert!(
        error[0].attr("message").contains("connection refused"),
        "{out}"
    );
    assert_eq!(error[0].attr("type"), "transport");
    // A skipped request is a skipped testcase.
    let cleanup = suites[2];
    assert_eq!(cleanup.attr("skipped"), "1");
    assert!(cleanup.all("testcase")[0].child("skipped").is_some());
    // Totals.
    assert_eq!(root.attr("tests"), "10");
    assert_eq!(root.attr("failures"), "1");
    assert_eq!(root.attr("errors"), "2");
    assert_eq!(root.attr("skipped"), "2");
    assert_eq!(root.attr("time"), "1.234");
}

#[test]
fn junit_names_without_iterations() {
    let one: Vec<_> = steps().into_iter().take(3).collect();
    let info = report::RunInfo {
        collection: "c".into(),
        iterations: 1,
    };
    let root = xml::parse(&report::junit(&info, &summary(), &one)).unwrap();
    assert_eq!(root.all("testsuite")[0].attr("name"), "users/get.yml");
}

#[test]
fn junit_escapes_everything() {
    let mut s = steps().remove(0);
    s.report.tests[0].name = "a\u{0}b\u{7}c\u{fffe}d\t\"e\"\r\n]]>".into();
    let out = report::junit(&info(), &summary(), &[s]);
    let root = xml::parse(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
    assert_eq!(
        root.all("testcase")[0].attr("name"),
        "a\\u0000b\\u0007c\\ufffed\t\"e\"\r\n]]>"
    );
}

#[test]
fn text_lines() {
    let steps = steps();
    let ok = report::text::step(&steps[0], "GET");
    assert!(ok.starts_with("✓ GET Get user  200  12 ms"), "{ok}");
    let failed = report::text::step(&steps[3], "GET");
    assert!(failed.starts_with("✗ GET Get user  200  12 ms"), "{failed}");
    assert!(
        failed.contains("\n    ✗ name is <\"Ann\" & 'Bob'>: expected"),
        "{failed}"
    );
    assert!(!failed.contains("status is 200"), "passed tests stay quiet");
    let error = report::text::step(&steps[1], "GET");
    assert!(error.starts_with("✗ GET Create user  "), "{error}");
    assert!(error.contains("connection refused"), "{error}");
    let skipped = report::text::step(&steps[2], "GET");
    assert!(skipped.starts_with("- GET Cleanup  skipped"), "{skipped}");
    let sum = report::text::summary(&summary());
    for needle in [
        "requests: 6 (3 passed, 3 failed, 2 skipped)",
        "tests: 3 passed, 1 failed",
        "assertions: 2 passed, 0 failed",
        "1.23 s",
    ] {
        assert!(sum.to_lowercase().contains(needle), "{needle}: {sum}");
    }
}

#[test]
fn exit_codes() {
    let all = steps();
    assert_eq!(exit_code(&summary(), &all), EXIT_TESTS);
    let passed: Vec<_> = all.iter().filter(|s| s.passed).cloned().collect();
    let ok = RunSummary {
        requests: 3,
        passed: 3,
        failed: 0,
        tests_failed: 0,
        ..summary()
    };
    assert_eq!(exit_code(&ok, &passed), EXIT_OK);
    // A failed assertion counts even if a step claims otherwise.
    let bad = RunSummary {
        assertions_failed: 1,
        ..ok.clone()
    };
    assert_eq!(exit_code(&bad, &passed), EXIT_TESTS);
}

// ---------------------------------------------------------------------------------------
// The binary

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rt-cli-run-{tag}-{}", std::process::id()));
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

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rt(cwd: &Path, args: &[&str]) -> Out {
    let data = std::env::temp_dir().join(format!("rt-cli-run-data-{}", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("run")
        .args(args)
        .current_dir(cwd)
        .env("RT_DATA_DIR", data)
        .output()
        .unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// A collection: `ping.yml` (GET /echo/ping), `users/` with `list.yml` (the secret `token` in
/// a header; not an echo, the response body is reported as received) and `get.yml`,
/// environment `Local` with `base` and the secret `token`.
fn collection(dir: &Path, base: &str) -> PathBuf {
    let root = dir.join("col");
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::create_dir_all(root.join("users")).unwrap();
    std::fs::write(
        root.join("users/folder.yml"),
        "info:\n  name: users\n  type: folder\n  seq: 2\n",
    )
    .unwrap();
    std::fs::write(
        root.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\ninfo:\n  name: run-test\n",
    )
    .unwrap();
    std::fs::write(
        root.join("environments/Local.yml"),
        format!(
            "name: Local\nvariables:\n  - name: base\n    value: {base}\n  - secret: true\n    name: token\n"
        ),
    )
    .unwrap();
    request(&root, "ping.yml", "Ping", 1, "{{base}}/echo/ping", "");
    request(
        &root,
        "users/list.yml",
        "List users",
        1,
        "{{base}}/status/200?id={{id}}",
        "  headers:\n    - name: Authorization\n      value: Bearer {{token}}\n",
    );
    request(
        &root,
        "users/get.yml",
        "Get user",
        2,
        "{{base}}/status/200",
        "",
    );
    root
}

fn request(root: &Path, file: &str, name: &str, seq: u32, url: &str, http_extra: &str) {
    std::fs::write(
        root.join(file),
        format!(
            "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{url}\"\n{http_extra}"
        ),
    )
    .unwrap();
}

fn server() -> (tokio::runtime::Runtime, String) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(rt_testserver::spawn_local());
    (rt, format!("http://{addr}"))
}

#[test]
fn usage_errors() {
    let tmp = TempDir::new("usage");
    // Not in a collection.
    let r = rt(&tmp.0, &[]);
    assert_eq!(r.code, EXIT_USAGE, "{}", r.stderr);
    assert!(r.stderr.contains("opencollection.yml"), "{}", r.stderr);
    let root = collection(&tmp.0, "http://127.0.0.1:9");
    let r = rt(&tmp.0, &["/definitely/not/here"]);
    assert_eq!(r.code, EXIT_USAGE);
    let r = rt(&root, &["-e", "Missing"]);
    assert_eq!(r.code, EXIT_USAGE);
    assert!(r.stderr.contains("Missing"), "{}", r.stderr);
    let r = rt(&root, &["--data", "nope.csv"]);
    assert_eq!(r.code, EXIT_USAGE);
    assert!(r.stderr.contains("nope.csv"), "{}", r.stderr);
    let r = rt(&root, &["ping.yml"]);
    assert_eq!(r.code, EXIT_USAGE);
    assert!(r.stderr.contains("rt send"), "{}", r.stderr);
    assert!(r.stdout.is_empty());
    // A collection without requests runs nothing: that is not a green build.
    let empty = tmp.0.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(
        empty.join("opencollection.yml"),
        "opencollection: \"1.0.0\"\n",
    )
    .unwrap();
    let r = rt(&empty, &[]);
    assert_eq!(r.code, EXIT_USAGE, "{}", r.stderr);
    assert!(r.stderr.contains("no requests"), "{}", r.stderr);
}

#[test]

fn runs_a_collection_with_reports() {
    let (_server, base) = server();
    let tmp = TempDir::new("all");
    let root = collection(&tmp.0, &base);
    let junit = tmp.0.join("out/junit.xml");
    let json = tmp.0.join("run.json");
    let r = rt(
        &root,
        &[
            "-e",
            "Local",
            "--var",
            "token=s3cr3t-run-token",
            "--var",
            "id=7",
            "--reporter",
            &format!("junit={}", junit.display()),
            "--reporter",
            "json",
            "--out",
            json.to_str().unwrap(),
        ],
    );
    assert_eq!(r.code, EXIT_OK, "{}", r.stderr);
    assert!(r.stdout.is_empty(), "reports went to files: {}", r.stdout);
    // Live text on stderr, in run order (seq, folders and requests by seq then name).
    let lines: Vec<_> = r.stderr.lines().filter(|l| l.starts_with('✓')).collect();
    assert_eq!(lines.len(), 3, "{}", r.stderr);
    assert!(
        lines[0].contains("Ping") && lines[0].contains("200"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.to_lowercase().contains("requests: 3 (3 passed"),
        "{}",
        r.stderr
    );
    // JSON: masked, complete.
    let text = std::fs::read_to_string(&json).unwrap();
    assert!(
        !text.contains("s3cr3t-run-token"),
        "secret in the JSON report"
    );
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["summary"]["passed"], 3);
    let paths: Vec<_> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["request_path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["ping.yml", "users/list.yml", "users/get.yml"]);
    let headers = &v["steps"][1]["report"]["request"]["headers"];
    assert!(
        headers
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h[1] == "Bearer ***"),
        "{headers}"
    );
    // JUnit: one suite per request, a passing "request" case when there are no tests.
    let x = std::fs::read_to_string(&junit).unwrap();
    assert!(!x.contains("s3cr3t-run-token"));
    let doc = xml::parse(&x).unwrap();
    assert_eq!(doc.all("testsuite").len(), 3);
    assert_eq!(doc.attr("failures"), "0");
}

#[test]

fn a_folder_runs_only_its_requests_and_junit_goes_to_stdout() {
    let (_server, base) = server();
    let tmp = TempDir::new("folder");
    let root = collection(&tmp.0, &base);
    let r = rt(
        &tmp.0,
        &[
            root.join("users").to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            "token=t",
            "--var",
            "id=1",
            "--reporter",
            "junit",
        ],
    );
    assert_eq!(r.code, EXIT_OK, "{}", r.stderr);
    let doc = xml::parse(&r.stdout).unwrap_or_else(|e| panic!("{e}\n{}", r.stdout));
    let names: Vec<_> = doc
        .all("testsuite")
        .iter()
        .map(|s| s.attr("name").to_owned())
        .collect();
    assert_eq!(names, ["users/list.yml", "users/get.yml"]);
}

#[test]

fn a_request_error_fails_the_run_and_bail_stops_it() {
    let tmp = TempDir::new("error");
    // Nothing listens on port 9 (discard) on loopback.
    let root = collection(&tmp.0, "http://127.0.0.1:9");
    let r = rt(
        &root,
        &["-e", "Local", "--var", "token=t", "--reporter", "json"],
    );
    assert_eq!(r.code, EXIT_TESTS, "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["summary"]["failed"], 3);
    assert!(r.stderr.contains('✗'), "{}", r.stderr);
    let r = rt(
        &root,
        &[
            "-e",
            "Local",
            "--var",
            "token=t",
            "--bail",
            "--reporter",
            "json",
        ],
    );
    assert_eq!(r.code, EXIT_TESTS);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["steps"].as_array().unwrap().len(), 1);
    assert_eq!(v["summary"]["stopped"], true);
}

#[test]

fn data_rows_are_iterations() {
    let (_server, base) = server();
    let tmp = TempDir::new("data");
    let root = collection(&tmp.0, &base);
    std::fs::write(tmp.0.join("rows.csv"), "id\n41\n42\n").unwrap();
    let data = tmp.0.join("rows.csv");
    let r = rt(
        &tmp.0,
        &[
            root.join("users").to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            "token=t",
            "--data",
            data.to_str().unwrap(),
            "--reporter",
            "junit",
        ],
    );
    assert_eq!(r.code, EXIT_OK, "{}", r.stderr);
    let doc = xml::parse(&r.stdout).unwrap();
    let names: Vec<_> = doc
        .all("testsuite")
        .iter()
        .map(|s| s.attr("name").to_owned())
        .collect();
    assert_eq!(
        names,
        [
            "users/list.yml [iteration 1]",
            "users/get.yml [iteration 1]",
            "users/list.yml [iteration 2]",
            "users/get.yml [iteration 2]",
        ]
    );
    // --iterations with data cycles the rows.
    let r = rt(
        &tmp.0,
        &[
            root.join("users/").to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            "token=t",
            "--data",
            data.to_str().unwrap(),
            "--iterations",
            "3",
            "--reporter",
            "json",
        ],
    );
    assert_eq!(r.code, EXIT_OK, "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    let urls: Vec<_> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["request_path"] == "users/list.yml")
        .map(|s| s["report"]["request"]["url"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(urls.len(), 3);
    assert!(urls[0].ends_with("id=41") && urls[1].ends_with("id=42") && urls[2].ends_with("id=41"));
}

fn tests_script(root: &Path) {
    std::fs::write(
        root.join("users/get.yml"),
        "info:\n  name: Get user\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/status/200\"\n\nruntime:\n  scripts:\n    - type: tests\n      code: |-\n        test(\"status is 200\", () => expect(rt.response.status).to.equal(200));\n        test(\"status is 201 & <odd>\", () => expect(rt.response.status).to.equal(201));\n",
    )
    .unwrap();
}

#[test]
#[ignore = "wave 7: needs strands A, D"]
fn failing_tests_exit_4_and_no_scripts_skips_them() {
    let (_server, base) = server();
    let tmp = TempDir::new("tests");
    let root = collection(&tmp.0, &base);
    tests_script(&root);
    let args = ["-e", "Local", "--var", "token=t", "--var", "id=1"];
    let r = rt(&root, &[&args[..], &["--reporter", "junit"]].concat());
    assert_eq!(r.code, EXIT_TESTS, "{}", r.stderr);
    assert!(
        r.stderr.contains("    ✗ status is 201 & <odd>"),
        "{}",
        r.stderr
    );
    let doc = xml::parse(&r.stdout).unwrap();
    let cases = doc.all("testcase");
    let failed: Vec<_> = cases
        .iter()
        .filter(|c| c.child("failure").is_some())
        .map(|c| c.attr("name"))
        .collect();
    assert_eq!(failed, ["status is 201 & <odd>"]);
    assert!(cases.iter().any(|c| c.attr("name") == "status is 200"));
    let r = rt(&root, &[&args[..], &["--no-scripts"]].concat());
    assert_eq!(r.code, EXIT_OK, "{}", r.stderr);
}

#[test]
fn failing_assertions_exit_4() {
    let (_server, base) = server();
    let tmp = TempDir::new("assert");
    let root = collection(&tmp.0, &base);
    std::fs::write(
        root.join("users/get.yml"),
        "info:\n  name: Get user\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/status/200\"\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"201\"\n",
    )
    .unwrap();
    let r = rt(
        &root.join("users"),
        &[
            "-e",
            "Local",
            "--var",
            "token=t",
            "--var",
            "id=1",
            "--reporter",
            "junit",
        ],
    );
    assert_eq!(r.code, EXIT_TESTS, "{}", r.stderr);
    let doc = xml::parse(&r.stdout).unwrap();
    let case = doc
        .all("testcase")
        .into_iter()
        .find(|c| c.attr("name") == "res.status eq 201")
        .expect("assertion testcase");
    assert!(case.child("failure").is_some());
}
