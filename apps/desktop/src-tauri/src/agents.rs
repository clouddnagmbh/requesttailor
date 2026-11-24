// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Tauri commands for agent policy, approvals (banner) and the audit log. Wave 8, strand C.
//!
//! - `agents_policy` / `agents_set_policy`: `extensions.requesttailor.agents` of the open
//!   collection; setting it is an unsaved edit like any other (`Workspace::set_agent_policy`).
//! - `agents_check`: effect and verdict of one request (`rt_app::agents::check`), for
//!   extensions that send on a model's behalf.
//! - `agents_pending`, `agents_decide`, `agents_audit`: the files `rt mcp` and `rt approve`
//!   share in the app's data folder (`rt_app::agents`). Approvals are machine-wide: the
//!   banner shows those of every collection, also when none is open. The webview polls
//!   `agents_pending` while the banner listens (`src/agents.ts`); reading the folder is cheap.
//! - Without a data folder (tests) nothing waits and the audit log is empty.

use std::path::Path;

use rt_app::AppError;
use rt_app::agents::{self, ApprovalDecision, AuditEntry, PendingApproval};
use rt_core::model::AgentPolicy;
use tauri::State;

use crate::commands::{AppCore, CommandError};

/// Most audit entries one call returns.
const AUDIT_MAX: usize = 1000;

fn io(e: std::io::Error) -> CommandError {
    AppError::Shell(e.to_string()).into()
}

#[tauri::command]
pub async fn agents_policy(core: State<'_, AppCore>) -> Result<AgentPolicy, CommandError> {
    Ok(core.agent_policy()?)
}

#[tauri::command]
pub async fn agents_set_policy(
    core: State<'_, AppCore>,
    policy: AgentPolicy,
) -> Result<(), CommandError> {
    Ok(core.set_agent_policy(&policy)?)
}

/// The agent rules for one request of the active collection, as edited in memory.
#[tauri::command]
pub async fn agents_check(
    core: State<'_, AppCore>,
    path: String,
    environment: Option<String>,
) -> Result<agents::AgentCheck, CommandError> {
    Ok(core.agent_check(&path, environment.as_deref())?)
}

/// The agent rules for a request that is not saved (wave 19).
#[tauri::command]
pub async fn agents_check_spec(
    core: State<'_, AppCore>,
    spec: rt_core::model::RequestSpec,
    environment: Option<String>,
) -> Result<agents::AgentCheck, CommandError> {
    Ok(core.agent_check_spec(&spec, environment.as_deref())?)
}

#[tauri::command]
pub async fn agents_pending(
    core: State<'_, AppCore>,
) -> Result<Vec<PendingApproval>, CommandError> {
    pending_in(core.data_dir.as_deref()).map_err(io)
}

#[tauri::command]
pub async fn agents_decide(
    core: State<'_, AppCore>,
    id: String,
    decision: ApprovalDecision,
) -> Result<(), CommandError> {
    decide_in(core.data_dir.as_deref(), &id, decision).map_err(io)
}

#[tauri::command]
pub async fn agents_audit(
    core: State<'_, AppCore>,
    limit: u32,
) -> Result<Vec<AuditEntry>, CommandError> {
    audit_in(core.data_dir.as_deref(), limit).map_err(io)
}

fn pending_in(data_dir: Option<&Path>) -> std::io::Result<Vec<PendingApproval>> {
    match data_dir {
        Some(dir) => agents::pending(dir),
        None => Ok(Vec::new()),
    }
}

fn decide_in(data_dir: Option<&Path>, id: &str, decision: ApprovalDecision) -> std::io::Result<()> {
    match data_dir {
        Some(dir) => agents::decide_as(dir, id, decision, "app"),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no pending approval {id}"),
        )),
    }
}

fn audit_in(data_dir: Option<&Path>, limit: u32) -> std::io::Result<Vec<AuditEntry>> {
    match data_dir {
        Some(dir) => agents::read_audit(dir, (limit as usize).min(AUDIT_MAX)),
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_app::agents::AuditOutcome;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rt-desktop-agents-{}-{}",
            std::process::id(),
            agents::new_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn approvals_and_audit_come_from_the_data_folder() {
        let dir = temp_dir();
        assert!(pending_in(Some(&dir)).unwrap().is_empty());
        let id = agents::new_id();
        agents::request_approval(
            &dir,
            &PendingApproval {
                id: id.clone(),
                at: 1,
                agent: "claude-code".into(),
                collection: "/c".into(),
                collection_name: "Orders".into(),
                request_path: "orders/create.yml".into(),
                environment: Some("PRD".into()),
                method: "POST".into(),
                url: "https://prd/orders".into(),
                reason: "writing request".into(),
            },
        )
        .unwrap();
        assert_eq!(pending_in(Some(&dir)).unwrap().len(), 1);
        decide_in(Some(&dir), &id, ApprovalDecision::Approved).unwrap();
        assert!(pending_in(Some(&dir)).unwrap().is_empty());
        assert_eq!(
            agents::decision(&dir, &id).unwrap(),
            Some((ApprovalDecision::Approved, "app".into()))
        );
        assert!(decide_in(Some(&dir), &id, ApprovalDecision::Denied).is_err());

        for n in 0..3 {
            agents::append_audit(
                &dir,
                &AuditEntry {
                    at: n,
                    agent: "a".into(),
                    tool: "list_requests".into(),
                    collection: "/c".into(),
                    request_path: None,
                    environment: None,
                    method: None,
                    url: None,
                    status: None,
                    outcome: AuditOutcome::Read,
                    approved_by: None,
                    rule: None,
                    detail: String::new(),
                },
            )
            .unwrap();
        }
        let audit = audit_in(Some(&dir), 2).unwrap();
        assert_eq!(audit.iter().map(|e| e.at).collect::<Vec<_>>(), [2, 1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_data_folder_nothing_waits() {
        assert!(pending_in(None).unwrap().is_empty());
        assert!(audit_in(None, 10).unwrap().is_empty());
        assert!(decide_in(None, "abcdef0123", ApprovalDecision::Approved).is_err());
    }
}
