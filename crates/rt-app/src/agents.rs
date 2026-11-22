// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Coding agents on a collection (wave 8): what `rt mcp` may do, approvals, audit log.
//!
//! The rules are [`AgentPolicy`](rt_core::model::AgentPolicy) in `opencollection.yml`. What
//! this module adds is machine-local and lives in the data folder (`RT_DATA_DIR`), shared by
//! `rt mcp`, `rt approve` and the desktop app:
//!
//! - `agents/approvals/<id>.json`: a [`PendingApproval`]; `rt mcp` writes it and waits.
//! - `agents/approvals/<id>.decision`: `approved` or `denied`, written by the desktop app
//!   (approval banner) or `rt approve <id>` / `rt deny <id>`. `rt mcp` removes both files
//!   when it has the answer or gives up (timeout).
//! - `agents/audit.jsonl`: one [`AuditEntry`] per tool call, appended, never rewritten.
//!   Masked like every report: no secret values, no bodies, no header values.
//!
//! An MCP client that supports elicitation may also be asked directly; the file-based way
//! always works, so the gate does not depend on the client.
//!
//! Owner: wave 8 interface. Implementation: strand C.

use std::path::{Path, PathBuf};

use rt_core::model::{AgentAccess, AgentPolicy, RequestSpec};
use serde::{Deserialize, Serialize};

/// Whether a request changes something on the server. Writing: every method other than
/// GET, HEAD, OPTIONS (and TRACE); a GraphQL body with a `mutation`; every gRPC call (the
/// file does not say whether it writes); a WebSocket with messages to send. SSE and
/// WebSocket without messages only listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Read,
    Write,
}

/// Strand C.
pub fn effect(_spec: &RequestSpec) -> Effect {
    Effect::Write
}

/// What the policy says for one send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum Verdict {
    Allowed,
    /// Writing request in a production environment.
    NeedsApproval,
    /// Refused with a reason the agent sees (e.g. "environment PRD allows only reading").
    Refused {
        reason: String,
    },
}

/// Strand C. `access: none` → refused; `read` + write effect → refused; `write` + production
/// → needs approval; otherwise allowed.
pub fn verdict(_policy: &AgentPolicy, _environment: Option<&str>, _effect: Effect) -> Verdict {
    Verdict::Refused {
        reason: "agents arrive in wave 8, strand C".into(),
    }
}

/// Environments an agent may see: all but those with access `none`.
pub fn visible(policy: &AgentPolicy, environment: &str) -> bool {
    policy.for_environment(Some(environment)).0 != AgentAccess::None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    /// Milliseconds since the Unix epoch.
    pub at: i64,
    /// MCP client name from `initialize` (e.g. `claude-code`), as the agent says.
    pub agent: String,
    /// Absolute path of the collection root.
    pub collection: String,
    pub collection_name: String,
    pub request_path: String,
    pub environment: Option<String>,
    pub method: String,
    /// Masked.
    pub url: String,
    /// Why approval is needed, for the banner.
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Approved,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditOutcome {
    /// Answered without sending (list tools, read a request).
    Read,
    Sent,
    Refused,
    Denied,
    /// No decision before the timeout; nothing was sent.
    TimedOut,
    Failed,
}

/// One line of `agents/audit.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub at: i64,
    pub agent: String,
    /// MCP tool name.
    pub tool: String,
    pub collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub outcome: AuditOutcome,
    /// `None` without approval; otherwise who decided (`app`, `cli`, `elicitation`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
    /// Masked; e.g. the refusal reason or the error.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

pub fn approvals_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("approvals")
}

pub fn audit_file(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join("audit.jsonl")
}

/// Pending approvals, oldest first (strand C).
pub fn pending(_data_dir: &Path) -> std::io::Result<Vec<PendingApproval>> {
    Ok(Vec::new())
}

/// Writes the decision for `id` (strand C). Unknown or already decided ids are an error.
pub fn decide(_data_dir: &Path, _id: &str, _decision: ApprovalDecision) -> std::io::Result<()> {
    Err(std::io::Error::other("agents arrive in wave 8, strand C"))
}

/// Appends one entry (strand C).
pub fn append_audit(_data_dir: &Path, _entry: &AuditEntry) -> std::io::Result<()> {
    Err(std::io::Error::other("agents arrive in wave 8, strand C"))
}

/// The last `limit` entries, newest first (strand C).
pub fn read_audit(_data_dir: &Path, _limit: usize) -> std::io::Result<Vec<AuditEntry>> {
    Ok(Vec::new())
}
