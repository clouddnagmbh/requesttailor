// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The two tabs: `integration` in the request editor (keep the log id of an answer, create the
//! check of its message processing log) and `integration-log` in the response view (a log
//! answer read as a table, with the way to its error text).

use rt_plugin_sdk::NoteLevel;
use rt_plugin_sdk::api::{
    ActionButton, Block, FieldKind, FormField, NewRequest, PluginResponse, TreeItem, View,
    ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::{Action, Error, Result, Tab};

use crate::mpl::{self, Entry, Kind};
use crate::requests::{self, LOG_FOLDER, LOG_VAR};

pub const REQUEST_TAB: &str = "integration";
pub const RESPONSE_TAB: &str = "integration-log";

pub fn render(tab: Tab) -> Result<ViewReply> {
    match tab.tab.as_str() {
        REQUEST_TAB => Ok(request_view(&tab.spec, None)),
        RESPONSE_TAB => Ok(response_view(tab.response.as_ref())),
        other => Err(Error::new(format!("unknown tab {other}"))),
    }
}

pub fn action(action: Action) -> Result<ViewReply> {
    match action.tab.as_str() {
        REQUEST_TAB => request_action(&action),
        RESPONSE_TAB => response_action(&action),
        other => Err(Error::new(format!("unknown tab {other}"))),
    }
}

fn view(blocks: Vec<Block>) -> ViewReply {
    ViewReply {
        view: View { blocks },
        ..ViewReply::default()
    }
}

fn looks_like_flow(url: &str) -> bool {
    url.contains("/http/") || url.contains("/cxf/")
}

fn request_view(spec: &RequestSpec, note: Option<(NoteLevel, String)>) -> ViewReply {
    let captured = requests::captures_log_id(spec);
    let mut blocks = vec![Block::Heading {
        text: "SAP Integration Suite".into(),
    }];
    if let Some((level, text)) = note {
        blocks.push(Block::Notice { level, text });
    }
    blocks.push(Block::Text {
        text: "An iFlow answers with the id of its message processing log. Keep that id, then check the log with an assertion on its status. The checks are ordinary assertions: they run in the runner and in rt run without this plugin.".into(),
    });
    blocks.push(Block::KeyValue {
        rows: vec![
            (
                "Looks like an iFlow endpoint".into(),
                yes(looks_like_flow(&spec.url)),
            ),
            (format!("Log id kept in {{{{{LOG_VAR}}}}}"), yes(captured)),
        ],
    });
    if !captured {
        blocks.push(Block::Actions {
            actions: vec![ActionButton {
                id: "capture".into(),
                label: "Keep the log id of each answer".into(),
            }],
        });
    }
    blocks.push(Block::Form {
        id: "check".into(),
        fields: vec![
            FormField {
                name: "expected".into(),
                label: "Expected status of the log".into(),
                kind: FieldKind::Select,
                value: "COMPLETED".into(),
                options: ["COMPLETED", "FAILED", "ESCALATED", "RETRY", "PROCESSING"]
                    .map(String::from)
                    .to_vec(),
            },
            FormField {
                name: "name".into(),
                label: "Name of the new request".into(),
                kind: FieldKind::Text,
                value: format!("Log of {}", spec.name),
                options: Vec::new(),
            },
        ],
        submit: "Create the log check".into(),
    });
    view(blocks)
}

fn yes(v: bool) -> String {
    if v { "yes" } else { "no" }.into()
}

fn request_action(action: &Action) -> Result<ViewReply> {
    let mut spec = action.spec.clone();
    match action.action.as_str() {
        "capture" => {
            if requests::captures_log_id(&spec) {
                return Ok(request_view(
                    &spec,
                    Some((NoteLevel::Info, "The log id is already kept.".into())),
                ));
            }
            spec.actions.push(requests::capture_action());
            let mut reply = request_view(
                &spec,
                Some((
                    NoteLevel::Info,
                    format!(
                        "Added the action that keeps the log id in {{{{{LOG_VAR}}}}} (unsaved)."
                    ),
                )),
            );
            reply.update_request = Some(spec);
            Ok(reply)
        }
        "check" => {
            let expected = action.input("expected").unwrap_or("COMPLETED").trim();
            if !matches!(
                expected,
                "COMPLETED" | "FAILED" | "ESCALATED" | "RETRY" | "PROCESSING"
            ) {
                return Err(Error::new(format!("{expected:?} is not a log status")));
            }
            let name = action.input("name").unwrap_or("").trim();
            let name = if name.is_empty() {
                format!("Log of {}", spec.name)
            } else {
                name.to_owned()
            };
            let check = requests::log_check(&name, &format!("{{{{{LOG_VAR}}}}}"), expected);
            let mut text = format!(
                "Added the request \"{name}\" to the folder {LOG_FOLDER} (unsaved). Run it after this request."
            );
            let mut update = None;
            if !requests::captures_log_id(&spec) {
                spec.actions.push(requests::capture_action());
                text.push_str(" This request now keeps the log id (unsaved).");
                update = Some(spec.clone());
            }
            let mut reply = request_view(&spec, Some((NoteLevel::Info, text)));
            reply.update_request = update;
            reply.create_requests = vec![NewRequest {
                folder: LOG_FOLDER.into(),
                name,
                spec: check,
            }];
            Ok(reply)
        }
        other => Err(Error::new(format!("unknown action {other}"))),
    }
}

fn entries_of(response: Option<&PluginResponse>) -> Option<Vec<Entry>> {
    response
        .and_then(|r| r.body.text.as_deref())
        .and_then(mpl::parse)
}

fn response_view(response: Option<&PluginResponse>) -> ViewReply {
    let Some(entries) = entries_of(response) else {
        return view(vec![Block::Notice {
            level: NoteLevel::Info,
            text: "This answer is not a message processing log. Send \"Log of the last trigger\" to see one.".into(),
        }]);
    };
    let mut blocks = vec![Block::Heading {
        text: "Message processing log".into(),
    }];
    match entries.as_slice() {
        [] => blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text: "No message processing log matches.".into(),
        }),
        [e] => {
            blocks.push(status_notice(e));
            blocks.push(Block::KeyValue { rows: rows(e) });
            if e.kind() == Kind::Failed {
                blocks.push(Block::Actions {
                    actions: vec![ActionButton {
                        id: format!("error:{}", e.guid),
                        label: "Create the request for the error text".into(),
                    }],
                });
            }
        }
        many => {
            blocks.push(Block::Table {
                columns: ["iFlow", "Status", "Started", "Took", "Message id"]
                    .map(String::from)
                    .to_vec(),
                rows: many
                    .iter()
                    .map(|e| {
                        vec![
                            e.flow.clone(),
                            e.status.clone(),
                            e.start_ms.map(mpl::time).unwrap_or_default(),
                            e.duration_ms().map(mpl::duration).unwrap_or_default(),
                            e.guid.clone(),
                        ]
                    })
                    .collect(),
            });
            let failed: Vec<TreeItem> = many
                .iter()
                .filter(|e| e.kind() == Kind::Failed)
                .map(|e| TreeItem {
                    label: format!("{} {}", e.flow, e.status),
                    detail: Some(e.guid.clone()),
                    children: Vec::new(),
                    action: Some(format!("error:{}", e.guid)),
                })
                .collect();
            if !failed.is_empty() {
                blocks.push(Block::Text {
                    text: "Click a failed message to create the request for its error text.".into(),
                });
                blocks.push(Block::Tree { nodes: failed });
            }
        }
    }
    view(blocks)
}

fn status_notice(e: &Entry) -> Block {
    let (level, text) = match e.kind() {
        Kind::Done => (
            NoteLevel::Info,
            format!("{}: the message was processed.", e.status),
        ),
        Kind::Running => (
            NoteLevel::Warning,
            format!(
                "{}: not finished yet. Send the request again in a moment.",
                e.status
            ),
        ),
        Kind::Failed => (
            NoteLevel::Error,
            format!("{}: the message failed.", e.status),
        ),
        Kind::Other => (NoteLevel::Info, format!("Status {}.", e.status)),
    };
    Block::Notice { level, text }
}

fn rows(e: &Entry) -> Vec<(String, String)> {
    let mut rows = vec![
        ("iFlow".to_owned(), e.flow.clone()),
        ("Status".to_owned(), e.status.clone()),
    ];
    if let Some(s) = e.start_ms {
        rows.push(("Started".into(), mpl::time(s)));
    }
    if let Some(d) = e.duration_ms() {
        rows.push(("Took".into(), mpl::duration(d)));
    }
    rows.push(("Message id".into(), e.guid.clone()));
    if !e.correlation_id.is_empty() {
        rows.push(("Correlation id".into(), e.correlation_id.clone()));
    }
    if !e.sender.is_empty() {
        rows.push(("Sender".into(), e.sender.clone()));
    }
    if !e.receiver.is_empty() {
        rows.push(("Receiver".into(), e.receiver.clone()));
    }
    rows
}

fn response_action(action: &Action) -> Result<ViewReply> {
    let Some(guid) = action.action.strip_prefix("error:") else {
        return Err(Error::new(format!("unknown action {}", action.action)));
    };
    let known = entries_of(action.response.as_ref())
        .is_some_and(|list| list.iter().any(|e| e.guid == guid));
    if !known {
        return Err(Error::new("that message is not in this answer"));
    }
    let name = format!("Error text of {guid}");
    let mut reply = response_view(action.response.as_ref());
    reply.view.blocks.insert(
        1,
        Block::Notice {
            level: NoteLevel::Info,
            text: format!("Added the request \"{name}\" to the folder {LOG_FOLDER} (unsaved)."),
        },
    );
    reply.create_requests = vec![NewRequest {
        folder: LOG_FOLDER.into(),
        name: name.clone(),
        spec: requests::error_text(&name, guid),
    }];
    Ok(reply)
}
