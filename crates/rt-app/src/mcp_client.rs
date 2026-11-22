// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Testing MCP servers like an API (wave 8): connect, list tools, call one, save the call as
//! a regression test and run saved tests.
//!
//! - Transports: `stdio` (start a local command; desktop and CLI only, not the web) and
//!   `http` (Streamable HTTP; the web app sends through `fetch` or the companion).
//! - Saved tests are companion files of the collection, `<name>.mcp.yaml` (the OpenCollection
//!   reader reads only `.yml`, so they never show up as requests). `{{variables}}` in the
//!   server config and the arguments are resolved from the selected environment; secrets are
//!   referenced by name and never written into the file.
//! - Results are masked like reports.
//!
//! Owner: wave 8 interface. Implementation: strand D (file format in
//! `rt-format/src/mcp_test.rs`).

use rt_core::model::Assertion;
use rt_core::script::AssertionResult;
use serde::{Deserialize, Serialize};

/// File extension of saved MCP tests.
pub const TEST_EXTENSION: &str = "mcp.yaml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum McpServerConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        /// Extra environment variables for the process; values may be `{{templates}}`.
        env: Vec<(String, String)>,
        /// Working directory, relative to the collection root; `None`: the root.
        cwd: Option<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// What `initialize` told us.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
    pub protocol_version: String,
    /// Capability names the server announced (`tools`, `resources`, `prompts`, …).
    pub capabilities: Vec<String>,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// JSON Schema of the arguments.
    pub input_schema: serde_json::Value,
    pub output_schema: Option<serde_json::Value>,
    /// `annotations` as sent (`readOnlyHint`, `destructiveHint`, …).
    pub annotations: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum McpContent {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        data: String,
    },
    Audio {
        mime_type: String,
        data: String,
    },
    Resource {
        uri: String,
        text: Option<String>,
    },
    /// Anything else, as sent.
    Other {
        value: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCallResult {
    pub content: Vec<McpContent>,
    pub structured: Option<serde_json::Value>,
    pub is_error: bool,
    pub time_ms: u64,
}

/// A saved call: one `<name>.mcp.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTest {
    pub name: String,
    pub server: McpServerConfig,
    pub tool: String,
    pub arguments: serde_json::Value,
    /// Evaluated against the result as a response: `res.body` is `structured` if present,
    /// else the JSON of the first text content (or the text); `res.status` is 200, or 500
    /// for `is_error`.
    pub assertions: Vec<Assertion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTestResult {
    /// Path of the test file relative to the collection root.
    pub path: String,
    pub passed: bool,
    pub result: Option<McpCallResult>,
    /// Connecting or calling failed (masked).
    pub error: Option<String>,
    pub assertions: Vec<AssertionResult>,
}
