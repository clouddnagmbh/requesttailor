// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What coding agents may do with a collection (wave 8, MCP server `rt mcp`).
//!
//! Stored in `extensions.requesttailor.agents` of `opencollection.yml` (OpenCollection has no
//! field for it), so the rules travel with the collection in Git:
//!
//! ```yaml
//! extensions:
//!   requesttailor:
//!     agents:
//!       access: read            # default for environments not listed (default: read)
//!       environments:
//!         Local: { access: write }
//!         PRD: { access: write, production: true }
//! ```
//!
//! Rules (enforced by `rt_app::agents`, strand C):
//! - `none`: the agent does not see the environment and cannot send with it.
//! - `read`: only reading requests (GET, HEAD, OPTIONS, a GraphQL query, a WebSocket or SSE
//!   that only listens); writing ones are refused.
//! - `write`: writing requests too. In a `production` environment every writing request
//!   waits for the user's approval, whatever `access` says.
//! - Agents never see secret values: tool results are masked reports, environments show
//!   secret variables by name only.
//!
//! Owner: wave 8 interface; reading and writing the extension: strand C.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AgentPolicy {
    /// For environments not listed in `environments`, and for sending without one.
    pub access: AgentAccess,
    pub environments: Vec<EnvironmentPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentPolicy {
    pub environment: String,
    pub access: AgentAccess,
    /// Marks a production system: writing requests need approval; the app shows it in red.
    pub production: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentAccess {
    None,
    #[default]
    Read,
    Write,
}

impl AgentPolicy {
    /// The rule for `environment` (`None`: sending without an environment).
    pub fn for_environment(&self, environment: Option<&str>) -> (AgentAccess, bool) {
        environment
            .and_then(|name| self.environments.iter().find(|e| e.environment == name))
            .map_or((self.access, false), |e| (e.access, e.production))
    }
}
