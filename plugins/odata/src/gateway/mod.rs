// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Gateway errors and `sap-statistics` (wave 18, strand J).
//!
//! - **Errors** (`response` hook, response tab `error`): an answer of 400 or more with an OData
//!   error body (V2 JSON and XML, V4 JSON) becomes a note with code, message and transaction
//!   ID, and a tab with the details, the resolution hints of the system, and where to look
//!   the transaction ID up ([`error`]).
//! - **Server time** (request tab `statistics`, `prepare` and `response` hooks): when the user
//!   switched "Measure server time" on for a service, `prepare` adds `sap-statistics: true`;
//!   the `response` hook reads the header of the answer into `NotesReply.segments` (gateway,
//!   application, database, the rest). The switch is kept per service root in the plugin's
//!   storage; without it a request is left as it is ([`statistics`]).
//!
//! Nothing here sends a request or needs a new right. The header goes only with requests the
//! user sends to the service itself.

pub mod error;
pub mod statistics;

use rt_plugin_sdk::api::{
    Block, Contributions, FieldKind, FormField, NotesReply, PluginContext, PluginRequest,
    PluginResponse, PrepareReply, TabContribution, TabPlace, View, ViewReply,
};
use rt_plugin_sdk::{Action, NoteLevel, Result, Tab, body_bytes, header, host, info, warning};
use serde_json::Value;

use crate::service;
use crate::write::util::{self, MASK};
use error::GatewayError;

/// The request header the server answers with `sap-statistics`.
pub const HEADER: &str = "sap-statistics";

/// The tab of the error details (a response tab) and the tab of the switch (a request tab).
pub const ERROR_TAB: &str = "error";
pub const STATISTICS_TAB: &str = "statistics";

/// Where to look an error up (unverified: SOURCES.md).
const LOOKUP: &str = "Look the transaction ID up in /IWFND/ERROR_LOG on the gateway system (and /IWBEP/ERROR_LOG for the backend).";

/// What this module adds to the manifest (the tabs; the hooks are already on). Keep it in step
/// with `plugin.json`: `tests/manifest.rs` compares both.
pub fn contributions() -> Contributions {
    Contributions {
        tabs: vec![
            TabContribution {
                id: ERROR_TAB.into(),
                title: "Error".into(),
                place: TabPlace::Response,
                content_types: vec![
                    "application/json".into(),
                    "application/xml".into(),
                    "text/xml".into(),
                ],
            },
            TabContribution {
                id: STATISTICS_TAB.into(),
                title: "Server time".into(),
                place: TabPlace::Request,
                content_types: Vec::new(),
            },
        ],
        ..Contributions::default()
    }
}

// -------------------------------------------------------------------------------------------
// Hooks

/// `prepare`: asks for the statistics when the user switched them on for this service.
pub fn prepare(request: &PluginRequest, context: &PluginContext) -> Result<PrepareReply> {
    let _ = context;
    let mut reply = PrepareReply::default();
    if header(&request.headers, HEADER).is_some() || request.url.contains(MASK) {
        return Ok(reply);
    }
    let Some(target) = util::split(&request.url) else {
        return Ok(reply);
    };
    if enabled(&target.service_root)? {
        reply.patch.set_headers.push((HEADER.into(), "true".into()));
        reply.notes.push(info(
            "sap-statistics: true (Measure server time is on for this service)",
        ));
    }
    Ok(reply)
}

/// `response`: the segments of `sap-statistics`, and a note for a Gateway error.
pub fn response(
    request: &PluginRequest,
    response: &PluginResponse,
    context: &PluginContext,
) -> Result<NotesReply> {
    let _ = (request, context);
    let mut reply = NotesReply::default();
    if let Some(value) = header(&response.headers, HEADER) {
        let stats = statistics::parse(value);
        if !stats.unknown.is_empty() {
            let shown = stats
                .unknown
                .iter()
                .map(|u| clip(u, 40))
                .collect::<Vec<_>>()
                .join(", ");
            reply.notes.push(if stats.segments.is_empty() {
                info(format!("sap-statistics: not understood: {shown}"))
            } else {
                info(format!("sap-statistics: ignored {shown}"))
            });
        }
        reply.segments = stats.segments;
    }
    if response.status >= 400
        && let Some(error) = error_of(response)
    {
        reply.notes.push(warning(error.summary()));
    }
    Ok(reply)
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}

fn error_of(response: &PluginResponse) -> Option<GatewayError> {
    let bytes = body_bytes(&response.body).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    error::parse(
        response
            .body
            .content_type
            .as_deref()
            .or_else(|| header(&response.headers, "content-type")),
        &text,
    )
}

// -------------------------------------------------------------------------------------------
// The switch, per service root

fn key(root: &str) -> String {
    service::storage_key("statistics", root)
}

fn enabled(root: &str) -> Result<bool> {
    Ok(host::storage_get(key(root))?.and_then(|v| v.as_bool()) == Some(true))
}

fn set_enabled(root: &str, on: bool) -> Result<()> {
    if on {
        host::storage_set(key(root), Value::Bool(true))
    } else {
        host::storage_delete(key(root))
    }
}

// -------------------------------------------------------------------------------------------
// Tabs

pub fn render(tab: Tab) -> Result<ViewReply> {
    match tab.tab.as_str() {
        ERROR_TAB => Ok(error_view(tab.response.as_ref())),
        _ => statistics_view(&tab.spec.url, tab.context.as_ref(), None),
    }
}

pub fn action(action: Action) -> Result<ViewReply> {
    let url = resolved(&action.spec.url, action.context.as_ref());
    let root = util::split(&url).map(|t| t.service_root);
    let mut saved = None;
    if action.action == "measure"
        && let Some(root) = &root
        && let Some(choice) = action.input("measure")
    {
        let on = choice == "true";
        set_enabled(root, on)?;
        saved = Some(on);
    }
    statistics_view(&action.spec.url, action.context.as_ref(), saved)
}

/// The URL with `{{name}}` filled in from the non-secret variables of the environment.
fn resolved(url: &str, context: Option<&PluginContext>) -> String {
    context.map_or_else(
        || url.to_owned(),
        |c| {
            c.variables.iter().fold(url.to_owned(), |u, (n, v)| {
                u.replace(&format!("{{{{{n}}}}}"), v)
            })
        },
    )
}

fn statistics_view(
    url: &str,
    context: Option<&PluginContext>,
    saved: Option<bool>,
) -> Result<ViewReply> {
    let url = resolved(url, context);
    let mut blocks = vec![Block::Heading {
        text: "Server time".into(),
    }];
    let root = (!url.contains("{{"))
        .then(|| util::split(&url))
        .flatten()
        .map(|t| t.service_root);
    match root {
        None => blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text: "The service of this request is not known yet: pick an environment so the URL \
                   resolves, then come back."
                .into(),
        }),
        Some(root) => {
            let on = enabled(&root)?;
            blocks.push(Block::KeyValue {
                rows: vec![("Service".into(), root)],
            });
            blocks.push(Block::Form {
                id: "measure".into(),
                fields: vec![FormField {
                    name: "measure".into(),
                    label: "Measure server time (adds the header sap-statistics: true to requests to this service)"
                        .into(),
                    kind: FieldKind::Checkbox,
                    value: on.to_string(),
                    options: Vec::new(),
                }],
                submit: "Apply".into(),
            });
            if let Some(now) = saved {
                blocks.push(Block::Notice {
                    level: NoteLevel::Info,
                    text: if now {
                        "On: the next send asks the server for its own time."
                    } else {
                        "Off: requests go out unchanged."
                    }
                    .into(),
                });
            }
        }
    }
    blocks.push(Block::Text {
        text: "What the server reports shows under the timing bar as \"as reported by the \
               server\". Its clock is not yours, so it never counts towards the total."
            .into(),
    });
    Ok(ViewReply {
        view: View { blocks },
        ..ViewReply::default()
    })
}

fn error_view(response: Option<&PluginResponse>) -> ViewReply {
    let blocks = match response {
        None => vec![Block::Text {
            text: "Send the request to see its answer here.".into(),
        }],
        Some(r) if r.status < 400 => vec![Block::Text {
            text: format!("The answer was {}: not an error.", r.status),
        }],
        Some(r) => match error_of(r) {
            None => vec![Block::Text {
                text: format!(
                    "The answer was {}, but its body is not an OData error (nothing to decode).",
                    r.status
                ),
            }],
            Some(e) => error_blocks(&e),
        },
    };
    ViewReply {
        view: View { blocks },
        ..ViewReply::default()
    }
}

fn error_blocks(e: &GatewayError) -> Vec<Block> {
    let mut blocks = vec![
        Block::Heading {
            text: "OData error".into(),
        },
        Block::Notice {
            level: NoteLevel::Warning,
            text: e.message.clone(),
        },
    ];
    let mut rows = Vec::new();
    if !e.code.is_empty() {
        rows.push(("Code".to_owned(), e.code.clone()));
    }
    rows.push(("Message".to_owned(), e.message.clone()));
    if let Some(target) = &e.target {
        rows.push(("Target".to_owned(), target.clone()));
    }
    if let Some(time) = e.time() {
        rows.push(("Time".to_owned(), time));
    }
    if let Some(id) = &e.transaction_id {
        rows.push(("Transaction ID".to_owned(), id.clone()));
    }
    blocks.push(Block::KeyValue { rows });
    if let Some(id) = &e.transaction_id {
        blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text: LOOKUP.into(),
        });
        blocks.push(Block::Code {
            language: None,
            text: id.clone(),
        });
    }
    if !e.details.is_empty() {
        blocks.push(Block::Heading {
            text: "Details".into(),
        });
        blocks.push(Block::Table {
            columns: ["Severity", "Code", "Message", "Target"]
                .map(String::from)
                .to_vec(),
            rows: e
                .details
                .iter()
                .map(|d| {
                    vec![
                        d.severity.clone().unwrap_or_default(),
                        d.code.clone(),
                        d.message.clone(),
                        d.target.clone().unwrap_or_default(),
                    ]
                })
                .collect(),
        });
    }
    let mut hints = Vec::new();
    if let Some(t) = &e.resolution.transaction {
        hints.push(("Transaction".to_owned(), t.clone()));
    }
    if let Some(n) = &e.resolution.note {
        hints.push(("SAP Note".to_owned(), n.clone()));
    }
    if !hints.is_empty() {
        blocks.push(Block::Heading {
            text: "Resolution (as the system says)".into(),
        });
        blocks.push(Block::KeyValue { rows: hints });
    }
    blocks
}
