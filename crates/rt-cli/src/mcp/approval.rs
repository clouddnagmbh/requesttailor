// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Waiting for the user's approval of a writing request against production.
//!
//! 1. The [`PendingApproval`] goes to `agents/approvals/<id>.json` in the data folder; the
//!    desktop app's banner and `rt approvals` show it.
//! 2. If the client announced `elicitation`, it is asked too (`elicitation/create` with one
//!    boolean `approve`). The first answer wins: the decision file (`rt approve`, the app)
//!    or the elicitation (`approved_by: elicitation`).
//! 3. Until then the file is polled every 100 ms and touched every few seconds (so the app
//!    knows the process still waits); after `--approval-timeout` seconds the call fails.
//! 4. Both files are removed in every case; an open elicitation is cancelled.

use std::time::Duration;

use rt_app::agents::{self, ApprovalDecision, PendingApproval};
use serde_json::{Value, json};

use super::server::{Polled, Server};

const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Approved by `by` (`app`, `cli`, `elicitation`, …).
    Approved(String),
    Denied(String),
    TimedOut,
    /// The client cancelled the call or went away.
    Cancelled,
    /// The approval could not be written.
    Failed(String),
}

fn elicitation_params(p: &PendingApproval) -> Value {
    let env = p.environment.as_deref().unwrap_or("no environment");
    json!({
        "message": format!(
            "{} wants to send {} {} ({}, {}). Approve this writing request?",
            p.agent, p.method, p.url, p.request_path, env
        ),
        "requestedSchema": {
            "type": "object",
            "properties": {
                "approve": {
                    "type": "boolean",
                    "title": "Approve",
                    "description": format!("Send {} {} against {env}", p.method, p.request_path),
                }
            },
            "required": ["approve"]
        }
    })
}

/// The decision in an answer to `elicitation/create`; `None`: no decision (cancelled,
/// error, malformed).
fn elicited(answer: &Value) -> Option<ApprovalDecision> {
    let result = answer.get("result")?;
    match result.get("action").and_then(Value::as_str)? {
        "accept" => Some(
            if result
                .get("content")
                .and_then(|c| c.get("approve"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Denied
            },
        ),
        "decline" => Some(ApprovalDecision::Denied),
        _ => None,
    }
}

/// Waits for the decision on `pending` (its `url` is already masked). `call_id` is the id of
/// the waiting `tools/call`.
pub async fn wait(server: &Server<'_>, call_id: &Value, pending: &PendingApproval) -> Outcome {
    let data = &server.data_dir;
    if let Err(e) = agents::request_approval(data, pending) {
        return Outcome::Failed(format!("cannot write the approval request: {e}"));
    }
    server.log(&format!(
        "waiting for approval {id}: {} {} ({}). Approve in the app or with `rt approve {id}`, \
         deny with `rt deny {id}`.",
        pending.method,
        pending.url,
        pending.environment.as_deref().unwrap_or("no environment"),
        id = pending.id,
    ));
    let mut asked = server
        .session
        .borrow()
        .elicitation
        .then(|| server.request_client("elicitation/create", elicitation_params(pending)));
    let timeout = Duration::from_secs(server.opts.approval_timeout);
    let started = web_time::Instant::now();
    let mut beat = web_time::Instant::now();
    let outcome = loop {
        match agents::decision(data, &pending.id) {
            Ok(Some((ApprovalDecision::Approved, by))) => break Outcome::Approved(by),
            Ok(Some((ApprovalDecision::Denied, by))) => break Outcome::Denied(by),
            Ok(None) => {}
            Err(e) => break Outcome::Failed(format!("cannot read the decision: {e}")),
        }
        match server.poll(call_id, asked.as_deref()) {
            Polled::Answer(answer) => {
                asked = None;
                match elicited(&answer) {
                    Some(ApprovalDecision::Approved) => {
                        break Outcome::Approved("elicitation".into());
                    }
                    Some(ApprovalDecision::Denied) => break Outcome::Denied("elicitation".into()),
                    // Dismissed: the other ways still work.
                    None => {}
                }
            }
            Polled::Cancelled => break Outcome::Cancelled,
            Polled::Nothing => {}
        }
        if started.elapsed() >= timeout {
            break Outcome::TimedOut;
        }
        if beat.elapsed() >= Duration::from_millis(agents::HEARTBEAT_MS) {
            let _ = agents::heartbeat(data, &pending.id);
            beat = web_time::Instant::now();
        }
        tokio::time::sleep(POLL).await;
    };
    if let Err(e) = agents::withdraw(data, &pending.id) {
        server.log(&format!("cannot remove approval {}: {e}", pending.id));
    }
    if let Some(id) = asked {
        server.send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": id, "reason": "answered elsewhere or timed out"}
        }));
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elicitation_answers() {
        let a = |v: Value| elicited(&json!({"jsonrpc": "2.0", "id": "rt-1", "result": v}));
        assert_eq!(
            a(json!({"action": "accept", "content": {"approve": true}})),
            Some(ApprovalDecision::Approved)
        );
        assert_eq!(
            a(json!({"action": "accept", "content": {"approve": false}})),
            Some(ApprovalDecision::Denied)
        );
        // Accept without a clear yes is no approval.
        assert_eq!(
            a(json!({"action": "accept", "content": {"approve": "yes"}})),
            Some(ApprovalDecision::Denied)
        );
        assert_eq!(a(json!({"action": "decline"})), Some(ApprovalDecision::Denied));
        assert_eq!(a(json!({"action": "cancel"})), None);
        assert_eq!(
            elicited(&json!({"jsonrpc": "2.0", "id": "rt-1", "error": {"code": -1, "message": "x"}})),
            None
        );
    }

    #[test]
    fn the_elicitation_names_request_and_environment() {
        let p = PendingApproval {
            id: agents::new_id(),
            at: 0,
            agent: "claude-code".into(),
            collection: "/c".into(),
            collection_name: "Orders".into(),
            request_path: "orders/create.yml".into(),
            environment: Some("PRD".into()),
            method: "POST".into(),
            url: "https://prd/orders".into(),
            reason: String::new(),
        };
        let params = elicitation_params(&p);
        let message = params["message"].as_str().unwrap();
        assert!(message.contains("POST https://prd/orders") && message.contains("PRD"));
        assert_eq!(params["requestedSchema"]["required"], json!(["approve"]));
        assert_eq!(
            params["requestedSchema"]["properties"]["approve"]["type"],
            "boolean"
        );
    }
}
