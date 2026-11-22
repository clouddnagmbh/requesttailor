// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The MCP client in the browser (Streamable HTTP only). Wave 8, strand D.
//!
//! ```text
//! Workspace.mcpConnect(server, input, companion) -> Promise<{ id, info }>
//! Workspace.mcpTools(id, companion) -> Promise<McpTool[]>
//! Workspace.mcpCall(id, tool, args, companion) -> Promise<McpCallResult>
//! Workspace.mcpDisconnect(id, companion) -> Promise<void>
//! Workspace.mcpTests() -> string[]
//! Workspace.mcpGetTest(path) -> McpTest
//! Workspace.mcpSaveTest(path | null, test, secretValues: string[]) -> string
//! Workspace.mcpRunTest(path, input, companion) -> Promise<McpTestResult>
//! ```
//!
//! `input` is `rt_app::SendInput` with the secret values (as for `send`), `companion`
//! `{ url, token }` or `null` for `fetch`, as for `send`. Values are the serde JSON of
//! `rt_app::mcp_client`. A stdio server rejects with an `AppError` of kind `unsupported`;
//! other failures with kind `shell` (masked). Connections are kept by id in this module
//! until `mcpDisconnect`; one call per connection at a time. Saved tests are staged in the
//! workspace (written by `prepareSave`/`commit`); `secretValues` are the stored values of
//! the collection's secret variables, which a test file must not contain.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use js_sys::Promise;
use rt_app::mcp_client::{self, McpClient, McpServerConfig, McpTest, McpVars};
use rt_app::{AppError, SendInput};
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use rt_core::transport::{HttpRequest, Response, Transport, TransportError};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::Companion;
use crate::workspace_js::{WorkspaceJs, app_error, from_js, int, to_js};

/// `fetch` or the companion, chosen per call like `Workspace.send`.
enum WebTransport {
    Fetch(FetchTransport),
    Companion(CompanionTransport),
}

impl WebTransport {
    fn from_js(companion: &JsValue) -> Result<Self, JsValue> {
        Ok(if companion.is_null() || companion.is_undefined() {
            Self::Fetch(FetchTransport::new())
        } else {
            let c: Companion = from_js(companion, "companion")?;
            Self::Companion(CompanionTransport::new(c.url, c.token))
        })
    }
}

impl Transport for WebTransport {
    async fn send(&self, request: HttpRequest) -> Result<Response, TransportError> {
        match self {
            Self::Fetch(t) => t.send(request).await,
            Self::Companion(t) => t.send(request).await,
        }
    }
}

thread_local! {
    static NEXT: Cell<u64> = const { Cell::new(0) };
    /// Open connections; `None` while a call runs.
    static OPEN: RefCell<HashMap<u64, Option<McpClient>>> = RefCell::new(HashMap::new());
}

fn take(id: u64) -> Result<McpClient, JsValue> {
    OPEN.with_borrow_mut(|open| match open.get_mut(&id) {
        Some(slot) => slot
            .take()
            .ok_or_else(|| app_error(AppError::Shell("the MCP connection is busy".into()))),
        None => Err(app_error(AppError::Shell(format!(
            "no MCP connection {id}"
        )))),
    })
}

fn put(id: u64, client: McpClient) {
    OPEN.with_borrow_mut(|open| {
        if let Some(slot) = open.get_mut(&id) {
            *slot = Some(client);
        }
    });
}

#[derive(Serialize)]
struct Connected {
    id: u64,
    info: rt_app::mcp_client::McpServerInfo,
}

fn mcp_error(e: mcp_client::McpError) -> JsValue {
    app_error(e.into())
}

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    /// See the module documentation.
    #[wasm_bindgen(js_name = mcpConnect)]
    pub fn mcp_connect(
        &self,
        server: JsValue,
        input: JsValue,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let server: McpServerConfig = from_js(&server, "server")?;
        let input: SendInput = from_js(&input, "input")?;
        let transport = WebTransport::from_js(&companion)?;
        let vars = McpVars::of_snapshot(&self.workspace().run_snapshot(input));
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let client = McpClient::connect(&transport, &server, vars, None)
                .await
                .map_err(mcp_error)?;
            let info = client.info().clone();
            let id = NEXT.with(|n| {
                n.set(n.get() + 1);
                n.get()
            });
            OPEN.with_borrow_mut(|open| open.insert(id, Some(client)));
            to_js(&Connected { id, info })
        }))
    }

    #[wasm_bindgen(js_name = mcpTools)]
    pub fn mcp_tools(&self, id: f64, companion: JsValue) -> Result<Promise, JsValue> {
        let id = int(id)?;
        let transport = WebTransport::from_js(&companion)?;
        let mut client = take(id)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let tools = client.tools(&transport).await;
            put(id, client);
            to_js(&tools.map_err(mcp_error)?)
        }))
    }

    #[wasm_bindgen(js_name = mcpCall)]
    pub fn mcp_call(
        &self,
        id: f64,
        tool: String,
        args: JsValue,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let id = int(id)?;
        let args: serde_json::Value = if args.is_undefined() {
            serde_json::json!({})
        } else {
            from_js(&args, "arguments")?
        };
        let transport = WebTransport::from_js(&companion)?;
        let mut client = take(id)?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let result = client.call(&transport, &tool, &args).await;
            put(id, client);
            to_js(&result.map_err(mcp_error)?)
        }))
    }

    #[wasm_bindgen(js_name = mcpDisconnect)]
    pub fn mcp_disconnect(&self, id: f64, companion: JsValue) -> Result<Promise, JsValue> {
        let id = int(id)?;
        let transport = WebTransport::from_js(&companion)?;
        let client = OPEN.with_borrow_mut(|open| open.remove(&id).flatten());
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            if let Some(client) = client {
                client.disconnect(&transport).await;
            }
            Ok(JsValue::UNDEFINED)
        }))
    }

    #[wasm_bindgen(js_name = mcpTests)]
    pub fn mcp_tests(&self) -> Result<JsValue, JsValue> {
        to_js(&mcp_client::tests(&self.workspace()))
    }

    #[wasm_bindgen(js_name = mcpGetTest)]
    pub fn mcp_get_test(&self, path: &str) -> Result<JsValue, JsValue> {
        to_js(&mcp_client::get_test(&self.workspace(), path).map_err(app_error)?)
    }

    #[wasm_bindgen(js_name = mcpSaveTest)]
    pub fn mcp_save_test(
        &self,
        path: Option<String>,
        test: JsValue,
        #[wasm_bindgen(js_name = secretValues)] secret_values: Vec<String>,
    ) -> Result<String, JsValue> {
        let test: McpTest = from_js(&test, "test")?;
        mcp_client::save_test(
            &mut self.workspace_mut(),
            path.as_deref(),
            &test,
            &secret_values,
        )
        .map_err(app_error)
    }

    #[wasm_bindgen(js_name = mcpRunTest)]
    pub fn mcp_run_test(
        &self,
        path: String,
        input: JsValue,
        companion: JsValue,
    ) -> Result<Promise, JsValue> {
        let input: SendInput = from_js(&input, "input")?;
        let transport = WebTransport::from_js(&companion)?;
        let test = mcp_client::get_test(&self.workspace(), &path).map_err(app_error)?;
        let vars = McpVars::of_snapshot(&self.workspace().run_snapshot(input));
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            to_js(&mcp_client::run_test(&transport, &path, &test, &vars, None).await)
        }))
    }
}
