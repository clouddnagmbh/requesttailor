// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Request tab `batch`: compose a `$batch` from operations, turn the request into
//! `POST <root>$batch`, and show the parts' results of the last send.
//!
//! The tab state holds the operations (`{"ops": [{"method", "url", "body", "changeset"}]}`).
//! Changeset `0` is a read outside any changeset; operations with the same changeset number
//! form one all-or-nothing changeset, in the order of their first appearance. Request tabs get
//! no response (`API-WUENSCHE.md`), so the `response` hook stores the parts' results per
//! request file (`batch-result:<request path>`) and the tab reads them from there.

use rt_plugin_sdk::api::{ActionButton, Block, FieldKind, FormField, View, ViewReply};
use rt_plugin_sdk::model::{Body, KeyValue, RawKind, RequestSpec};
use rt_plugin_sdk::{Action, NoteLevel, Result, Tab, host};
use serde::{Deserialize, Serialize};

use super::batch::{self, Item, Part};
use super::{etag, util};
use crate::edm::Version;
use crate::service;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    pub method: String,
    /// Relative to the service root.
    pub url: String,
    #[serde(default)]
    pub body: String,
    /// `0`: outside a changeset.
    #[serde(default)]
    pub changeset: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub ops: Vec<Op>,
}

const METHODS: &[&str] = &["GET", "POST", "PATCH", "PUT", "MERGE", "DELETE"];

fn variables(context: Option<&rt_plugin_sdk::api::PluginContext>) -> Vec<(String, String)> {
    context.map(|c| c.variables.clone()).unwrap_or_default()
}

fn spec_body(spec: &RequestSpec) -> String {
    match &spec.body {
        Body::Raw { data, .. } => data.clone(),
        _ => String::new(),
    }
}

fn spec_header<'a>(spec: &'a RequestSpec, name: &str) -> Option<&'a str> {
    spec.headers
        .iter()
        .find(|h| h.enabled && h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// The operations of a request that already is a `$batch` (to show it when the tab opens).
fn ops_of_batch(spec: &RequestSpec) -> Vec<Op> {
    let Some(ct) = spec_header(spec, "Content-Type") else {
        return Vec::new();
    };
    let Some(items) = batch::parse_request(ct, &spec_body(spec)) else {
        return Vec::new();
    };
    let mut ops = Vec::new();
    let mut changeset = 0;
    for item in items {
        match item {
            Item::Read(p) => ops.push(op_of(p, 0)),
            Item::Changeset(parts) => {
                changeset += 1;
                ops.extend(parts.into_iter().map(|p| op_of(p, changeset)));
            }
        }
    }
    ops
}

fn op_of(part: Part, changeset: u32) -> Op {
    Op {
        method: part.method,
        url: part.url,
        body: part.body.unwrap_or_default(),
        changeset,
    }
}

fn state_of(value: Option<&serde_json::Value>, spec: &RequestSpec) -> State {
    match value.and_then(|v| serde_json::from_value::<State>(v.clone()).ok()) {
        Some(state) => state,
        None => State {
            ops: ops_of_batch(spec),
        },
    }
}

/// Items in batch order: reads in place, changesets at the position of their first operation.
pub fn items(ops: &[Op]) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut changesets: Vec<(u32, usize)> = Vec::new();
    for op in ops {
        let part = Part {
            method: op.method.to_ascii_uppercase(),
            url: op.url.trim().trim_start_matches('/').to_owned(),
            headers: Vec::new(),
            body: (!op.body.trim().is_empty()).then(|| op.body.clone()),
        };
        if op.changeset == 0 {
            items.push(Item::Read(part));
        } else if let Some((_, at)) = changesets.iter().find(|(c, _)| *c == op.changeset) {
            if let Item::Changeset(parts) = &mut items[*at] {
                parts.push(part);
            }
        } else {
            changesets.push((op.changeset, items.len()));
            items.push(Item::Changeset(vec![part]));
        }
    }
    items
}

fn view(
    state: &State,
    spec: &RequestSpec,
    request_path: &str,
    notice: Option<(NoteLevel, String)>,
) -> Result<ViewReply> {
    let mut blocks = vec![Block::Heading {
        text: "$batch".into(),
    }];
    if let Some((level, text)) = notice {
        blocks.push(Block::Notice { level, text });
    }
    match util::template_target(&spec.url, &[]) {
        Some(t) => blocks.push(Block::KeyValue {
            rows: vec![("Service root".into(), t.root)],
        }),
        None => blocks.push(Block::Notice {
            level: NoteLevel::Warning,
            text: "The request URL is not an OData URL; set it to the service (or one of its entity sets) first.".into(),
        }),
    }
    if state.ops.is_empty() {
        blocks.push(Block::Text {
            text: "No operations yet. Add the current request or fill in the form below.".into(),
        });
    } else {
        blocks.push(Block::Table {
            columns: vec![
                "#".into(),
                "Changeset".into(),
                "Method".into(),
                "URL".into(),
                "Body".into(),
            ],
            rows: state
                .ops
                .iter()
                .enumerate()
                .map(|(i, op)| {
                    let mut body = op.body.replace(['\r', '\n'], " ");
                    if body.chars().count() > 60 {
                        body = body.chars().take(57).collect::<String>() + "...";
                    }
                    vec![
                        (i + 1).to_string(),
                        if op.changeset == 0 {
                            "-".into()
                        } else {
                            op.changeset.to_string()
                        },
                        op.method.clone(),
                        op.url.clone(),
                        body,
                    ]
                })
                .collect(),
        });
    }
    blocks.push(Block::Actions {
        actions: vec![
            button("add-current", "Add current request"),
            button("build", "Build $batch request"),
            button("remove-last", "Remove last"),
            button("clear", "Clear"),
        ],
    });
    let next_changeset = state
        .ops
        .iter()
        .map(|o| o.changeset)
        .max()
        .unwrap_or(0)
        .max(1);
    blocks.push(Block::Form {
        id: "add".into(),
        fields: vec![
            FormField {
                name: "method".into(),
                label: "Method".into(),
                kind: FieldKind::Select,
                value: "POST".into(),
                options: METHODS.iter().map(|m| (*m).to_owned()).collect(),
            },
            text_field(
                "url",
                "URL (relative to the service root, $1 = result of operation 1)",
                "",
            ),
            text_field("body", "JSON body", ""),
            text_field(
                "changeset",
                "Changeset (0 = none, for reads)",
                &next_changeset.to_string(),
            ),
        ],
        submit: "Add operation".into(),
    });
    blocks.push(Block::Text {
        text: "Operations with the same changeset number are applied all-or-nothing. Content-IDs count the operations from 1, so $1 in a URL refers to what operation 1 created (only within its changeset). If-Match comes from remembered ETags; the CSRF token is fetched when the batch is sent.".into(),
    });
    if let Some(result) = host::storage_get(service::storage_key("batch-result", request_path))? {
        blocks.push(Block::Heading {
            text: "Last response".into(),
        });
        let rows = result["rows"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|r| {
                        vec![
                            r["n"].to_string(),
                            r["changeset"]
                                .as_u64()
                                .map_or("-".into(), |c| c.to_string()),
                            r["operation"].as_str().unwrap_or_default().to_owned(),
                            r["status"].to_string(),
                            r["message"].as_str().unwrap_or_default().to_owned(),
                        ]
                    })
                    .collect()
            })
            .unwrap_or_default();
        blocks.push(Block::Table {
            columns: vec![
                "Part".into(),
                "Changeset".into(),
                "Operation".into(),
                "Status".into(),
                "Message".into(),
            ],
            rows,
        });
    }
    Ok(ViewReply {
        view: View { blocks },
        state: Some(serde_json::to_value(state)?),
        ..ViewReply::default()
    })
}

fn button(id: &str, label: &str) -> ActionButton {
    ActionButton {
        id: id.into(),
        label: label.into(),
    }
}

fn text_field(name: &str, label: &str, value: &str) -> FormField {
    FormField {
        name: name.into(),
        label: label.into(),
        kind: FieldKind::Text,
        value: value.into(),
        options: Vec::new(),
    }
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let state = state_of(tab.state.as_ref(), &tab.spec);
    view(&state, &tab.spec, &tab.target.request_path, None)
}

pub fn action(action: Action) -> Result<ViewReply> {
    let mut state = state_of(action.state.as_ref(), &action.spec);
    let spec = &action.spec;
    let path = &action.target.request_path;
    let vars = variables(action.context.as_ref());
    let notice = |level, text: &str| Some((level, text.to_owned()));
    match action.action.as_str() {
        "add-current" => {
            let Some(t) = util::template_target(&spec.url, &[]) else {
                return view(
                    &state,
                    spec,
                    path,
                    notice(NoteLevel::Warning, "The request URL is not an OData URL."),
                );
            };
            if t.resource == "$batch" {
                return view(
                    &state,
                    spec,
                    path,
                    notice(NoteLevel::Warning, "The request is already a $batch."),
                );
            }
            let method = spec.method.to_ascii_uppercase();
            let query = t
                .query
                .split('&')
                .filter(|p| {
                    !p.is_empty()
                        && !p.starts_with("sap-client=")
                        && !p.starts_with("sap-language=")
                })
                .collect::<Vec<_>>()
                .join("&");
            let url = if query.is_empty() {
                t.resource
            } else {
                format!("{}?{query}", t.resource)
            };
            let changeset = if method == "GET" {
                0
            } else {
                state
                    .ops
                    .iter()
                    .map(|o| o.changeset)
                    .max()
                    .unwrap_or(0)
                    .max(1)
            };
            state.ops.push(Op {
                method,
                url,
                body: spec_body(spec),
                changeset,
            });
            view(&state, spec, path, None)
        }
        "add" => {
            let url = action.input("url").unwrap_or_default().trim().to_owned();
            if url.is_empty() {
                return view(
                    &state,
                    spec,
                    path,
                    notice(NoteLevel::Warning, "The operation needs a URL."),
                );
            }
            let method = action.input("method").unwrap_or("GET").to_ascii_uppercase();
            let changeset = match action.input("changeset").map(str::trim) {
                None | Some("") => u32::from(method != "GET"),
                Some(c) => match c.parse() {
                    Ok(c) => c,
                    Err(_) => {
                        return view(
                            &state,
                            spec,
                            path,
                            notice(NoteLevel::Warning, "The changeset is a number (0 = none)."),
                        );
                    }
                },
            };
            let body = action.input("body").unwrap_or_default().to_owned();
            if !body.trim().is_empty() && serde_json::from_str::<serde_json::Value>(&body).is_err()
            {
                return view(
                    &state,
                    spec,
                    path,
                    notice(NoteLevel::Warning, "The body is not valid JSON."),
                );
            }
            state.ops.push(Op {
                method,
                url,
                body,
                changeset,
            });
            view(&state, spec, path, None)
        }
        "remove-last" => {
            state.ops.pop();
            view(&state, spec, path, None)
        }
        "clear" => view(&State::default(), spec, path, None),
        "build" => build(state, spec, path, &vars),
        other => Err(rt_plugin_sdk::Error::new(format!("unknown action {other}"))),
    }
}

fn build(
    state: State,
    spec: &RequestSpec,
    path: &str,
    vars: &[(String, String)],
) -> Result<ViewReply> {
    let Some(t) = util::template_target(&spec.url, vars) else {
        return view(
            &state,
            spec,
            path,
            Some((
                NoteLevel::Warning,
                "The request URL is not an OData URL.".into(),
            )),
        );
    };
    if state.ops.is_empty() {
        return view(
            &state,
            spec,
            path,
            Some((NoteLevel::Warning, "Add operations first.".into())),
        );
    }
    if let Some(op) = state
        .ops
        .iter()
        .find(|o| o.changeset == 0 && o.method != "GET")
    {
        let text = format!(
            "{} {} is outside a changeset; OData V2 allows only reads there.",
            op.method, op.url
        );
        if t.version != Some(Version::V4) {
            return view(&state, spec, path, Some((NoteLevel::Warning, text)));
        }
    }
    let mut items = items(&state.ops);
    // If-Match from remembered ETags, where the operation has an entity URL.
    let mut with_etag = 0;
    if let Some(root) = &t.resolved_root {
        for item in &mut items {
            let parts = match item {
                Item::Read(p) => std::slice::from_mut(p),
                Item::Changeset(ps) => ps.as_mut_slice(),
            };
            for part in parts {
                if !matches!(part.method.as_str(), "PATCH" | "PUT" | "MERGE" | "DELETE")
                    || part.url.starts_with('$')
                {
                    continue;
                }
                let url = format!("{root}{}", part.url);
                if let Some(e) = etag::lookup(&url)?.filter(|e| !e.contains(util::MASK)) {
                    part.headers.push(("If-Match".into(), e));
                    with_etag += 1;
                }
            }
        }
    }
    let version = t
        .resolved_root
        .as_deref()
        .and_then(util::model)
        .map(|m| m.version)
        .or(t.version)
        .unwrap_or(Version::V4);
    let boundary = format!(
        "batch_{:08x}",
        util::hash(&serde_json::to_string(&state.ops)?) as u32
    );
    let (body, content_type) = batch::build(&items, version, &boundary);

    let mut new = spec.clone();
    new.method = "POST".into();
    new.url = format!("{}$batch{}", t.root, util::sap_query(&t.query));
    new.params = util::sap_params(&spec.params);
    new.headers.retain(|h| {
        ![
            "content-type",
            "accept",
            "if-match",
            "x-csrf-token",
            "x-http-method",
        ]
        .contains(&h.name.to_ascii_lowercase().as_str())
    });
    new.headers
        .push(KeyValue::new("Content-Type", content_type));
    new.headers.push(KeyValue::new("Accept", "multipart/mixed"));
    new.body = Body::Raw {
        kind: RawKind::Text,
        data: body,
    };
    let changesets = items
        .iter()
        .filter(|i| matches!(i, Item::Changeset(_)))
        .count();
    let mut text = format!(
        "The request is now POST $batch with {} operations ({} changesets). Send it to see the results here.",
        state.ops.len(),
        changesets
    );
    if with_etag > 0 {
        text.push_str(&format!(
            " If-Match from remembered ETags on {with_etag} operations."
        ));
    }
    let mut reply = view(&state, &new, path, Some((NoteLevel::Info, text)))?;
    reply.update_request = Some(new);
    Ok(reply)
}
