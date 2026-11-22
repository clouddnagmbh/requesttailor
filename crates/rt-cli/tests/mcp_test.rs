// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt mcp-test` (wave 8, strand D): the built binary runs saved MCP tests against
//! rt-testserver's MCP server, over Streamable HTTP and over stdio (this test binary is the
//! stdio server: `mcp_stdio_child` serves when `RT_MCP_STDIO_CHILD` is set).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rt_cli::{Command as Cli, EXIT_OK, EXIT_TESTS, EXIT_TRANSPORT, EXIT_USAGE, parse_args};
use serde_json::Value;

const SECRET: &str = "mcp-cli-secret-7d1c";

/// Not a test: the stdio MCP server, when this binary is started as one.
#[test]
fn mcp_stdio_child() {
    if std::env::var_os("RT_MCP_STDIO_CHILD").is_some() {
        rt_testserver::mcp::serve_stdio();
    }
}

/// rt-testserver's MCP endpoint on a random port, served by a thread for the test's life.
fn http_server() -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let addr = rt_testserver::spawn_mcp("127.0.0.1:0".parse().unwrap())
                .await
                .unwrap();
            tx.send(addr).unwrap();
            std::future::pending::<()>().await;
        });
    });
    format!("http://{}/mcp", rx.recv().unwrap())
}

fn yaml_string(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// A collection with an environment that points at `url` and has a secret `token` (its
/// value comes from `--var`, as in a pipeline).
fn collection(name: &str, url: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rt-mcp-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("environments")).unwrap();
    std::fs::create_dir_all(dir.join("mcp")).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: MCP\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("environments/Local.yml"),
        format!(
            "name: Local\nvariables:\n  - name: mcpUrl\n    value: {}\n  - secret: true\n    name: token\n",
            yaml_string(url)
        ),
    )
    .unwrap();
    dir
}

fn http_test(dir: &Path, file: &str, tool: &str, arguments: &str, assertions: &str) {
    let text = format!(
        "name: {tool}\nserver:\n  transport: http\n  url: \"{{{{mcpUrl}}}}\"\n  headers:\n    - name: Authorization\n      value: \"Bearer {{{{token}}}}\"\ntool: {tool}\narguments:{arguments}\n{assertions}"
    );
    std::fs::write(dir.join(file), text).unwrap();
}

fn rt(dir: &Path, args: &[&str]) -> (i32, Value, String) {
    let out: Output = Command::new(env!("CARGO_BIN_EXE_rt"))
        .arg("mcp-test")
        .args(args)
        .current_dir(dir)
        .env("RT_DATA_DIR", dir.join(".data"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!stdout.contains(SECRET), "{stdout}");
    assert!(!stderr.contains(SECRET), "{stderr}");
    let json = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap_or(-1), json, stderr)
}

#[test]
fn parses_arguments() {
    match parse_args([
        "mcp-test",
        "a.mcp.yaml",
        "mcp",
        "-e",
        "Local",
        "--var",
        "x=1=2",
    ]) {
        Ok(Cli::McpTest(a)) => {
            assert_eq!(a.paths, [PathBuf::from("a.mcp.yaml"), PathBuf::from("mcp")]);
            assert_eq!(a.environment.as_deref(), Some("Local"));
            assert_eq!(a.vars, [("x".to_owned(), "1=2".to_owned())]);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(parse_args(["mcp-test", "--help"]), Ok(Cli::Help)));
    assert!(parse_args(["mcp-test", "--var", "novalue"]).is_err());
    assert!(parse_args(["mcp-test", "--nope"]).is_err());
}

#[test]
fn runs_http_tests_and_exits_4_when_one_fails() {
    let url = http_server();
    let dir = collection("http", &url);
    http_test(
        &dir,
        "mcp/add.mcp.yaml",
        "add",
        "\n  a: 2\n  b: 3",
        "assertions:\n  - expression: res.body.sum\n    operator: eq\n    value: \"5\"\n",
    );
    http_test(
        &dir,
        "mcp/secret.mcp.yaml",
        "secret",
        " {}",
        "assertions:\n  - expression: res.body\n    operator: eq\n    value: \"Bearer {{token}}\"\n",
    );
    let var = format!("token={SECRET}");

    let (code, json, stderr) = rt(&dir, &["-e", "Local", "--var", &var]);
    assert_eq!(code, EXIT_OK, "{stderr}\n{json}");
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 0);
    let tests = json["tests"].as_array().unwrap();
    assert_eq!(tests[0]["path"], "mcp/add.mcp.yaml");
    assert_eq!(tests[0]["result"]["structured"]["sum"], 5);
    assert_eq!(tests[1]["result"]["content"][0]["text"], "Bearer ***");
    assert!(stderr.contains("PASS mcp/add.mcp.yaml"), "{stderr}");

    // One path only; a failing assertion exits 4.
    http_test(
        &dir,
        "mcp/wrong.mcp.yaml",
        "add",
        "\n  a: 2\n  b: 3",
        "assertions:\n  - expression: res.body.sum\n    operator: eq\n    value: \"6\"\n",
    );
    let (code, json, stderr) = rt(&dir, &["mcp/wrong.mcp.yaml", "-e", "Local", "--var", &var]);
    assert_eq!(code, EXIT_TESTS, "{stderr}");
    assert_eq!(json["tests"].as_array().unwrap().len(), 1);
    assert_eq!(json["tests"][0]["assertions"][0]["actual"], "5");
    assert!(stderr.contains("FAIL mcp/wrong.mcp.yaml"), "{stderr}");

    // A folder of the collection, from another working directory.
    let (code, json, _) = rt(
        &std::env::temp_dir(),
        &[
            dir.join("mcp").to_str().unwrap(),
            "-e",
            "Local",
            "--var",
            &var,
        ],
    );
    assert_eq!(code, EXIT_TESTS);
    assert_eq!(json["tests"].as_array().unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runs_stdio_tests() {
    let dir = collection("stdio", "unused");
    let exe = std::env::current_exe().unwrap();
    std::fs::write(
        dir.join("mcp/echo.mcp.yaml"),
        format!(
            "name: echo\nserver:\n  transport: stdio\n  command: {}\n  args:\n    - --exact\n    - mcp_stdio_child\n    - --nocapture\n  env:\n    - name: RT_MCP_STDIO_CHILD\n      value: \"1\"\n    - name: MCP_TOKEN\n      value: \"{{{{token}}}}\"\ntool: secret\narguments: {{}}\nassertions:\n  - expression: res.body\n    operator: eq\n    value: \"{{{{token}}}}\"\n",
            yaml_string(&exe.display().to_string())
        ),
    )
    .unwrap();
    let var = format!("token={SECRET}");
    let (code, json, stderr) = rt(&dir, &["-e", "Local", "--var", &var]);
    assert_eq!(code, EXIT_OK, "{stderr}\n{json}");
    assert_eq!(json["tests"][0]["result"]["content"][0]["text"], "***");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn connect_failures_exit_1_and_usage_errors_2() {
    // Nothing listens on port 9.
    let dir = collection("down", "http://127.0.0.1:9/mcp");
    http_test(&dir, "mcp/down.mcp.yaml", "echo", " {}", "");
    let (code, json, stderr) = rt(&dir, &["-e", "Local"]);
    assert_eq!(code, EXIT_TRANSPORT, "{stderr}");
    assert!(
        json["tests"][0]["error"]
            .as_str()
            .unwrap()
            .contains("cannot connect")
    );

    let (code, _, stderr) = rt(&dir, &["-e", "Nope"]);
    assert_eq!(code, EXIT_USAGE, "{stderr}");
    let (code, _, stderr) = rt(&dir, &["environments/Local.yml"]);
    assert_eq!(code, EXIT_USAGE, "{stderr}");
    std::fs::remove_file(dir.join("mcp/down.mcp.yaml")).unwrap();
    let (code, _, stderr) = rt(&dir, &[]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("no MCP tests"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Against strand C's `rt mcp` (a stdio MCP server of the collection): `list_requests` with
/// an assertion on its result.
#[test]
#[ignore = "wave 8: needs strand C"]
fn tests_rt_mcp_itself() {
    let dir = collection("rt-mcp", "unused");
    std::fs::write(
        dir.join("health.yml"),
        "info:\n  name: Health\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{mcpUrl}}/health\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("mcp/rt.mcp.yaml"),
        format!(
            "name: rt mcp lists requests\nserver:\n  transport: stdio\n  command: {}\n  args:\n    - mcp\ntool: list_requests\narguments: {{}}\nassertions:\n  - expression: res.status\n    operator: eq\n    value: \"200\"\n",
            yaml_string(env!("CARGO_BIN_EXE_rt"))
        ),
    )
    .unwrap();
    let (code, json, stderr) = rt(&dir, &[]);
    assert_eq!(code, EXIT_OK, "{stderr}\n{json}");
    let text = json["tests"][0]["result"].to_string();
    assert!(text.contains("health.yml"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
