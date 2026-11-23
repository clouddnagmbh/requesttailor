// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The MCP demo server through the real client: built in (`rt-demo://mcp`), over Streamable
//! HTTP on a loopback port (`LocalServer`) and over stdio (this test binary as the server
//! process, `demo_stdio_child`). The same checks for all three.

use rt_app::mcp_client::{McpClient, McpContent, McpServerConfig, McpTest, McpVars, run_test};
use rt_app::mcp_demo::serve::{LocalServer, serve_stdio};
use rt_app::mcp_demo::{DEMO_URL, SERVER_NAME};
use rt_core::model::{Assertion, Collection};
use rt_core::transport::native::NativeTransport;
use serde_json::json;

/// Not a test: the stdio demo server, when this binary is started as one.
#[test]
fn demo_stdio_child() {
    if std::env::var_os("RT_MCP_DEMO_CHILD").is_some() {
        let stdin = std::io::stdin();
        serve_stdio(stdin.lock(), std::io::stdout()).unwrap();
    }
}

fn stdio_server() -> McpServerConfig {
    McpServerConfig::Stdio {
        command: std::env::current_exe().unwrap().display().to_string(),
        args: vec![
            "--exact".into(),
            "demo_stdio_child".into(),
            "--nocapture".into(),
        ],
        env: vec![("RT_MCP_DEMO_CHILD".into(), "1".into())],
        cwd: None,
    }
}

fn http(url: &str) -> McpServerConfig {
    McpServerConfig::Http {
        url: url.into(),
        headers: Vec::new(),
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, f)
}

fn vars() -> McpVars {
    McpVars::new(
        &Collection::default(),
        None,
        &[("city".into(), "Oslo".into())],
    )
}

async fn exercise(server: McpServerConfig) {
    let t = NativeTransport::new().unwrap();
    let mut client = McpClient::connect(&t, &server, vars(), None).await.unwrap();
    let info = client.info().clone();
    assert_eq!(info.name, SERVER_NAME);
    assert_eq!(
        info.capabilities,
        ["logging", "prompts", "resources", "tools"]
    );

    // Three pages of tools, two of resources, two of prompts: the client follows cursors.
    let tools = client.tools(&t).await.unwrap();
    assert_eq!(tools.len(), 10);
    assert!(
        tools
            .iter()
            .any(|t| t.name == "fake_weather" && t.output_schema.is_some())
    );
    let resources = client.resources(&t).await.unwrap();
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(
        uris,
        ["demo://readme", "demo://cities.json", "demo://todos"]
    );
    assert_eq!(resources[0].mime_type.as_deref(), Some("text/markdown"));
    let readme = client.read_resource(&t, "demo://readme").await.unwrap();
    assert!(readme[0].text.as_deref().unwrap().contains("fake_weather"));
    let prompts = client.prompts(&t).await.unwrap();
    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[0].name, "summarize");
    assert!(prompts[0].arguments[0].required);
    let prompt = client
        .get_prompt(&t, "summarize", &[("text".into(), "Hello {{city}}".into())])
        .await
        .unwrap();
    assert!(
        matches!(&prompt.messages[0].content, McpContent::Text { text } if text.ends_with("Hello Oslo"))
    );

    // Variables in arguments; the same weather everywhere.
    let weather = client
        .call(&t, "fake_weather", &json!({ "city": "{{city}}" }))
        .await
        .unwrap();
    assert_eq!(
        weather.structured,
        Some(
            json!({ "city": "Oslo", "temperature": 17, "units": "metric", "conditions": "thunderstorms", "humidity": 70, "wind": 4 })
        )
    );

    // State per session.
    client
        .call(&t, "todo_add", &json!({ "title": "Try the demo" }))
        .await
        .unwrap();
    let list = client.call(&t, "todo_list", &json!({})).await.unwrap();
    assert_eq!(list.structured.unwrap()["open"], 1);

    // Progress and logs arrive before the result and are kept with it.
    let countdown = client
        .call(&t, "countdown", &json!({ "from": 2 }))
        .await
        .unwrap();
    let methods: Vec<&str> = countdown
        .notifications
        .iter()
        .map(|n| n.method.as_str())
        .collect();
    assert_eq!(
        methods,
        [
            "notifications/progress",
            "notifications/message",
            "notifications/progress",
            "notifications/message"
        ]
    );
    assert_eq!(countdown.notifications[2].params["progress"], 2);
    assert!(client.call(&t, "fail", &json!({})).await.unwrap().is_error);
    client.disconnect(&t).await;

    // A new connection is a new session: the list is empty again.
    let mut again = McpClient::connect(&t, &server, vars(), None).await.unwrap();
    let list = again.call(&t, "todo_list", &json!({})).await.unwrap();
    assert_eq!(list.structured.unwrap()["open"], 0);
    again.disconnect(&t).await;
}

#[test]
fn built_in() {
    block_on(exercise(http(DEMO_URL)));
}

#[test]
fn streamable_http_on_a_local_port() {
    let server = LocalServer::start(0).unwrap();
    assert!(server.url().starts_with("http://127.0.0.1:"));
    let url = server.url();
    block_on(exercise(http(&url)));
    server.stop();
    // Stopped: nothing answers any more.
    let t = NativeTransport::new().unwrap();
    let result = block_on(McpClient::connect(&t, &http(&url), vars(), None));
    assert!(result.is_err());
}

#[test]
fn stdio() {
    block_on(exercise(stdio_server()));
}

#[test]
fn saved_tests_against_the_built_in_server_need_nothing_running() {
    let test = McpTest {
        name: "Weather in Vienna".into(),
        server: http(DEMO_URL),
        tool: "fake_weather".into(),
        arguments: json!({ "city": "Vienna" }),
        assertions: vec![Assertion {
            expression: "res.body.conditions".into(),
            operator: "eq".into(),
            value: "\"sunny\"".into(),
            description: None,
            enabled: true,
        }],
    };
    let t = NativeTransport::new().unwrap();
    let result = block_on(run_test(&t, "mcp/w.mcp.yaml", &test, &vars(), None));
    assert!(result.passed, "{result:?}");
}
