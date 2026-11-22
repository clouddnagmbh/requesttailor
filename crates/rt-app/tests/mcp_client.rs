// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Acceptance tests of the MCP client (wave 8, strand D) against rt-testserver's MCP server:
//! Streamable HTTP through the native transport, and stdio with this test binary as the
//! server process (`mcp_stdio_child` runs `rt_testserver::mcp::serve_stdio` when started
//! with `RT_MCP_STDIO_CHILD=1`).

use rt_app::mcp_client::{
    McpClient, McpContent, McpError, McpServerConfig, McpTest, McpVars, run_test,
};
use rt_core::model::{Assertion, Collection, Environment, Variable};
use rt_core::transport::native::NativeTransport;
use serde_json::json;

const SECRET: &str = "tok-3f9a-very-secret";

/// Not a test: the stdio MCP server, when this binary is started as one.
#[test]
fn mcp_stdio_child() {
    if std::env::var_os("RT_MCP_STDIO_CHILD").is_some() {
        rt_testserver::mcp::serve_stdio();
    }
}

fn stdio_server() -> McpServerConfig {
    McpServerConfig::Stdio {
        command: std::env::current_exe().unwrap().display().to_string(),
        args: vec![
            "--exact".into(),
            "mcp_stdio_child".into(),
            "--nocapture".into(),
        ],
        env: vec![
            ("RT_MCP_STDIO_CHILD".into(), "1".into()),
            ("MCP_TOKEN".into(), "Bearer {{token}}".into()),
        ],
        cwd: None,
    }
}

fn vars(url: &str) -> McpVars {
    let mut collection = Collection::default();
    collection.environments.push(Environment {
        name: "Local".into(),
        variables: vec![
            Variable::new("mcpUrl", url),
            Variable {
                name: "token".into(),
                value: Some(SECRET.into()),
                enabled: true,
                secret: true,
            },
        ],
        client_certificates: Vec::new(),
        external_secrets: None,
    });
    McpVars::new(
        &collection,
        Some("Local"),
        &[("word".into(), "hello".into())],
    )
}

fn http_server() -> McpServerConfig {
    McpServerConfig::Http {
        url: "{{mcpUrl}}".into(),
        headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, f)
}

async fn http_url() -> String {
    let addr = rt_testserver::spawn_mcp("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    format!("http://{addr}/mcp")
}

/// Connect, list (two pages), call (JSON and SSE answers), errors, masking: the same for
/// both transports.
async fn exercise(server: McpServerConfig, vars: McpVars, http: bool) {
    let t = NativeTransport::new().unwrap();
    let mut client = McpClient::connect(&t, &server, vars, None).await.unwrap();
    let info = client.info().clone();
    assert_eq!(info.name, "rt-testserver");
    assert_eq!(info.protocol_version, "2025-11-25");
    assert_eq!(info.capabilities, ["logging", "tools"]);
    assert_eq!(
        info.instructions.as_deref(),
        Some("Test server for RequestTailor.")
    );

    let tools = client.tools(&t).await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["echo", "add", "fail", "secret", "headers", "image"]);
    assert_eq!(tools[0].title.as_deref(), Some("Echo"));
    assert_eq!(tools[0].annotations, Some(json!({ "readOnlyHint": true })));
    assert!(tools[1].output_schema.is_some());

    let echo = client
        .call(&t, "echo", &json!({ "text": "{{word}} world" }))
        .await
        .unwrap();
    assert_eq!(
        echo.content,
        [McpContent::Text {
            text: "hello world".into()
        }]
    );
    assert!(!echo.is_error);

    let add = client
        .call(&t, "add", &json!({ "a": 2, "b": 3 }))
        .await
        .unwrap();
    assert_eq!(add.structured, Some(json!({ "sum": 5 })));

    let fail = client.call(&t, "fail", &json!({})).await.unwrap();
    assert!(fail.is_error);

    let image = client.call(&t, "image", &json!({})).await.unwrap();
    assert!(
        matches!(&image.content[0], McpContent::Image { mime_type, .. } if mime_type == "image/png")
    );

    // The secret reaches the server, never the result.
    let secret = client.call(&t, "secret", &json!({})).await.unwrap();
    assert_eq!(
        secret.content,
        [McpContent::Text {
            text: "Bearer ***".into()
        }]
    );

    // JSON-RPC errors are errors, masked like everything else.
    let err = client
        .call(&t, "nope", &json!({ "x": SECRET }))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        McpError::Rpc {
            code: -32602,
            message: "Unknown tool: nope".into()
        }
    );

    if http {
        // Session id and negotiated version go with every request after `initialize`.
        let headers = client.call(&t, "headers", &json!({})).await.unwrap();
        let h = headers.structured.unwrap();
        assert_eq!(h["protocol_version"], "2025-11-25");
        assert!(h["session_id"].as_str().unwrap().starts_with("rt-session-"));
        assert_eq!(h["accept"], "application/json, text/event-stream");
    }
    client.disconnect(&t).await;
}

#[test]
fn streamable_http() {
    block_on(async {
        let url = http_url().await;
        exercise(http_server(), vars(&url), true).await;
    });
}

#[test]
fn stdio() {
    block_on(async {
        exercise(stdio_server(), vars("unused"), false).await;
    });
}

#[test]
fn http_sessions_end_on_disconnect() {
    block_on(async {
        let url = http_url().await;
        let t = NativeTransport::new().unwrap();
        let client = McpClient::connect(&t, &http_server(), vars(&url), None)
            .await
            .unwrap();
        let mut again = McpClient::connect(&t, &http_server(), vars(&url), None)
            .await
            .unwrap();
        client.disconnect(&t).await;
        // The other session still works.
        assert!(again.tools(&t).await.is_ok());
        again.disconnect(&t).await;
    });
}

#[test]
fn connection_failures_are_clear_and_masked() {
    block_on(async {
        let t = NativeTransport::new().unwrap();
        // Nothing listens there; the URL carries the secret.
        let server = McpServerConfig::Http {
            url: "http://127.0.0.1:9/mcp?key={{token}}".into(),
            headers: Vec::new(),
        };
        let err = McpClient::connect(&t, &server, vars(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Connect(_)), "{err:?}");
        assert!(!err.to_string().contains(SECRET), "{err}");

        // Not an MCP endpoint.
        let base = rt_testserver::spawn_local().await;
        let server = McpServerConfig::Http {
            url: format!("http://{base}/status/500"),
            headers: Vec::new(),
        };
        let err = McpClient::connect(&t, &server, vars(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Http { status: 500, .. }), "{err:?}");

        // A command that does not exist.
        let server = McpServerConfig::Stdio {
            command: "rt-no-such-command-{{token}}".into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
        };
        let err = McpClient::connect(&t, &server, vars(""), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        assert!(!err.to_string().contains(SECRET), "{err}");

        // A process that exits right away: its stderr is in the message.
        let server = McpServerConfig::Stdio {
            command: std::env::current_exe().unwrap().display().to_string(),
            args: vec!["--definitely-not-a-flag".into()],
            env: Vec::new(),
            cwd: None,
        };
        let err = McpClient::connect(&t, &server, vars(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::Closed(_)), "{err:?}");
    });
}

fn assertion(expression: &str, operator: &str, value: &str) -> Assertion {
    Assertion {
        expression: expression.into(),
        operator: operator.into(),
        value: value.into(),
        description: None,
        enabled: true,
    }
}

#[test]
fn saved_tests_run_with_assertions() {
    block_on(async {
        let url = http_url().await;
        let t = NativeTransport::new().unwrap();
        let v = vars(&url);
        let test = |tool: &str, args, assertions| McpTest {
            name: tool.into(),
            server: http_server(),
            tool: tool.into(),
            arguments: args,
            assertions,
        };
        let add = test(
            "add",
            json!({ "a": 2, "b": 3 }),
            vec![
                assertion("res.status", "eq", "200"),
                assertion("res.body.sum", "eq", "5"),
            ],
        );
        let r = run_test(&t, "mcp/add.mcp.yaml", &add, &v, None).await;
        assert!(r.passed, "{r:?}");
        assert_eq!(r.assertions.len(), 2);
        assert_eq!(r.path, "mcp/add.mcp.yaml");

        let wrong = test(
            "add",
            json!({ "a": 2, "b": 3 }),
            vec![assertion("res.body.sum", "eq", "6")],
        );
        let r = run_test(&t, "w.mcp.yaml", &wrong, &v, None).await;
        assert!(!r.passed);
        assert_eq!(r.assertions[0].actual.as_deref(), Some("5"));

        // Without assertions: passes unless isError; an expected failure can be asserted.
        let fail = test("fail", json!({}), Vec::new());
        assert!(!run_test(&t, "f.mcp.yaml", &fail, &v, None).await.passed);
        let expected = test(
            "fail",
            json!({}),
            vec![assertion("res.status", "eq", "500")],
        );
        assert!(run_test(&t, "f.mcp.yaml", &expected, &v, None).await.passed);
        let echo = test(
            "echo",
            json!({ "text": "hi" }),
            vec![assertion("res.body", "eq", "hi")],
        );
        assert!(run_test(&t, "e.mcp.yaml", &echo, &v, None).await.passed);

        // A JSON-RPC error fails the test with the (masked) message.
        let unknown = test("nope", json!({}), Vec::new());
        let r = run_test(&t, "n.mcp.yaml", &unknown, &v, None).await;
        assert!(!r.passed);
        assert!(r.error.unwrap().contains("Unknown tool"));

        // The secret is masked in results and assertion values.
        let secret = test(
            "secret",
            json!({}),
            vec![assertion("res.body", "eq", "Bearer {{token}}")],
        );
        let r = run_test(&t, "s.mcp.yaml", &secret, &v, None).await;
        assert!(r.passed, "{r:?}");
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains(SECRET), "{json}");
    });
}
