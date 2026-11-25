// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The `response` hook: plain-words notes for the answers of an iFlow endpoint and of the
//! message processing log API.

use rt_plugin_sdk::api::{NotesReply, PluginNote, PluginRequest, PluginResponse};
use rt_plugin_sdk::{header, info, warning};

use crate::mpl::{self, Entry, Kind, LOG_ID_HEADER};

pub fn respond(request: &PluginRequest, response: &PluginResponse) -> NotesReply {
    let mut notes: Vec<PluginNote> = Vec::new();
    if let Some(id) = header(&response.headers, LOG_ID_HEADER) {
        notes.push(info(format!(
            "Message processing log ID {id}. Read its status with the request \"Log of the last trigger\" (the Integration tab has the steps)."
        )));
    }
    if let Some(entries) = response.body.text.as_deref().and_then(mpl::parse) {
        notes.extend(log_notes(&entries));
    }
    if matches!(response.status, 401 | 403) {
        notes.push(warning(access_hint(&request.url, response.status)));
    }
    NotesReply {
        notes,
        ..NotesReply::default()
    }
}

fn access_hint(url: &str, status: u16) -> String {
    let what = if status == 401 {
        "the token was not accepted"
    } else {
        "the token lacks a role"
    };
    if url.contains("/api/v1/") {
        format!(
            "{status}: {what}. The management API needs a client of the API plan with a monitoring role (for example MonitoringDataRead)."
        )
    } else if url.contains("/http/") || url.contains("/cxf/") {
        format!(
            "{status}: {what}. An iFlow endpoint needs a runtime client with the role ESBMessaging.send."
        )
    } else {
        format!("{status}: {what}.")
    }
}

fn log_notes(entries: &[Entry]) -> Vec<PluginNote> {
    match entries {
        [] => vec![info("No message processing log matches.")],
        [one] => vec![one_note(one)],
        many => {
            let failed = many.iter().filter(|e| e.kind() == Kind::Failed).count();
            let running = many.iter().filter(|e| e.kind() == Kind::Running).count();
            let text = format!(
                "{} message processing logs: {failed} failed, {running} still running.",
                many.len(),
            );
            vec![if failed > 0 {
                warning(text)
            } else {
                info(text)
            }]
        }
    }
}

fn one_note(e: &Entry) -> PluginNote {
    let took = e
        .duration_ms()
        .map(|d| format!(" after {}", mpl::duration(d)))
        .unwrap_or_default();
    match e.kind() {
        Kind::Done => info(format!("{} ended {}{took}.", flow(e), e.status)),
        Kind::Running => warning(format!(
            "{} is still {}. The log is written while the message runs: send this request again in a moment.",
            flow(e),
            e.status
        )),
        Kind::Failed => warning(format!(
            "{} ended {}{took}. The text of the error is at .../MessageProcessingLogs('{}')/ErrorInformation/$value (the Message log tab creates the request).",
            flow(e),
            e.status,
            e.guid
        )),
        Kind::Other => info(format!("{} has status {}{took}.", flow(e), e.status)),
    }
}

fn flow(e: &Entry) -> String {
    if e.flow.is_empty() {
        "The message".into()
    } else {
        format!("iFlow {}", e.flow)
    }
}
