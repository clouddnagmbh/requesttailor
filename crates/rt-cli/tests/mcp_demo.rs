// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt mcp-demo` (wave 14): the built binary serves the demo on stdio and on a loopback port,
//! and `rt mcp-test` runs a saved test against the built-in demo (`rt-demo://mcp`) with
//! nothing started.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use rt_app::mcp_client::{McpClient, McpServerConfig, McpVars};
use rt_cli::{Command as Cli, EXIT_OK, parse_args};
use rt_core::model::Collection;
use rt_core::transport::native::NativeTransport;
use serde_json::{Value, json};

fn rt() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rt"))
}

#[test]
fn is_a_command_with_help() {
    assert!(matches!(
        parse_args(["mcp-demo", "--http", "--port", "48401"]),
        Ok(Cli::McpDemo(rt_cli::mcp_demo::Args {
            http: true,
            port: 48401
        }))
    ));
    let out = rt().arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("rt mcp-demo [--http] [--port <PORT>]"),
        "{help}"
    );
    assert!(help.contains("rt-demo://mcp"));
}

#[test]
fn serves_on_stdio() {
    let mut child = rt()
        .arg("mcp-demo")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut ask = |message: Value| -> Value {
        writeln!(stdin, "{message}").unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };
    let init = ask(
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-11-25" } }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "requesttailor-demo");
    let weather = ask(
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "fake_weather", "arguments": { "city": "Vienna" } } }),
    );
    assert_eq!(
        weather["result"]["structuredContent"]["conditions"],
        "sunny"
    );
    let not_an_object = ask(json!("x"));
    assert_eq!(not_an_object["error"]["code"], -32600);
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn serves_streamable_http_on_loopback() {
    let mut child = rt()
        .args(["mcp-demo", "--http", "--port", "48402"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    assert!(line.contains("http://127.0.0.1:48402/mcp"), "{line}");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = tokio::task::LocalSet::new().block_on(&runtime, async {
        let t = NativeTransport::new().unwrap();
        let server = McpServerConfig::Http {
            url: "http://127.0.0.1:48402/mcp".into(),
            headers: Vec::new(),
        };
        let vars = McpVars::new(&Collection::default(), None, &[]);
        let mut client = McpClient::connect_with(&t, &server, vars, None, Duration::from_secs(10))
            .await
            .unwrap();
        let tools = client.tools(&t).await.unwrap();
        let r = client
            .call(&t, "countdown", &json!({ "from": 2 }))
            .await
            .unwrap();
        client.disconnect(&t).await;
        (tools.len(), r.notifications.len())
    });
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(result, (10, 4));
}

#[test]
fn mcp_test_runs_saved_tests_against_the_built_in_demo() {
    let dir = std::env::temp_dir().join(format!("rt-mcp-demo-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mcp")).unwrap();
    std::fs::write(
        dir.join("opencollection.yml"),
        "opencollection: 1.0.0\ninfo:\n  name: Demo\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("mcp/weather.mcp.yaml"),
        "name: Weather in Vienna\nserver:\n  transport: http\n  url: rt-demo://mcp\ntool: fake_weather\narguments:\n  city: Vienna\nassertions:\n  - expression: res.status\n    operator: eq\n    value: \"200\"\n  - expression: res.body.conditions\n    operator: eq\n    value: '\"sunny\"'\n",
    )
    .unwrap();
    let out = rt().arg("mcp-test").current_dir(&dir).output().unwrap();
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(EXIT_OK), "{report}");
    assert_eq!(report["passed"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}
