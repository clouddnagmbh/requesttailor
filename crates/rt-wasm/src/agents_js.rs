// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What coding agents may do with the open collection, in the browser (wave 8, strand C):
//! `extensions.requesttailor.agents` of `opencollection.yml`.
//!
//! ```text
//! Workspace.agentPolicy() -> AgentPolicy
//! Workspace.setAgentPolicy(policy)          unsaved edit, like updateEnvironment
//! Workspace.agentCheck(path, environment) -> AgentCheck   effect and verdict of one request
//! ```
//!
//! Approvals and the audit log belong to `rt mcp` on the user's machine; the web app has
//! neither (`apps/web/src/agents.ts`).

use rt_core::model::AgentPolicy;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{WorkspaceJs, app_error, from_js, to_js};

#[wasm_bindgen(js_class = Workspace)]
impl WorkspaceJs {
    #[wasm_bindgen(js_name = agentPolicy)]
    pub fn agent_policy(&self) -> Result<JsValue, JsValue> {
        to_js(&self.workspace().collection().config.agents)
    }

    /// `AgentCheck` for the request at `path` as it is in memory, with `environment` (`null`:
    /// none): the same `rt_app::agents::check` the other shells call.
    #[wasm_bindgen(js_name = agentCheck)]
    pub fn agent_check(&self, path: &str, environment: Option<String>) -> Result<JsValue, JsValue> {
        let check =
            rt_app::agents::check(self.workspace().collection(), path, environment.as_deref())
                .map_err(app_error)?;
        to_js(&check)
    }

    #[wasm_bindgen(js_name = setAgentPolicy)]
    pub fn set_agent_policy(&self, policy: JsValue) -> Result<(), JsValue> {
        let policy: AgentPolicy = from_js(&policy, "policy")?;
        self.workspace_mut()
            .set_agent_policy(&policy)
            .map_err(app_error)
    }
}
