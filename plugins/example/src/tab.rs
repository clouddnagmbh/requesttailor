// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The response tab `summary`: what came back, as key-value and table blocks, and an action
//! that adds body statistics. Whether they are shown lives in the tab state, so the plugin
//! stays stateless.

use rt_plugin_sdk::api::{ActionButton, Block, PluginResponse, View, ViewReply};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::{Action, NoteLevel, Result, Tab, bail};
use serde_json::json;

const TAB: &str = "summary";

pub fn render(tab: Tab) -> Result<ViewReply> {
    check_tab(&tab.tab)?;
    let stats = tab
        .state
        .as_ref()
        .and_then(|s| s.get("stats"))
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    view(&tab.spec, tab.response.as_ref(), stats)
}

pub fn action(action: Action) -> Result<ViewReply> {
    check_tab(&action.tab)?;
    let stats = match action.action.as_str() {
        "body-stats" => true,
        "hide-stats" => false,
        other => bail!("unknown action: {other}"),
    };
    view(&action.spec, action.response.as_ref(), stats)
}

fn check_tab(tab: &str) -> Result<()> {
    if tab != TAB {
        bail!("unknown tab: {tab}");
    }
    Ok(())
}

fn view(spec: &RequestSpec, response: Option<&PluginResponse>, stats: bool) -> Result<ViewReply> {
    let mut blocks = vec![Block::Heading {
        text: format!("{} {}", spec.method, spec.url),
    }];
    let Some(response) = response else {
        blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text: "Send the request to see a summary of its response.".into(),
        });
        return Ok(reply(blocks, false));
    };

    let body = rt_plugin_sdk::body_bytes(&response.body)?;
    blocks.push(Block::KeyValue {
        rows: vec![
            ("Status".into(), response.status.to_string()),
            (
                "Content type".into(),
                crate::content_type(response).unwrap_or("none").into(),
            ),
            ("Size".into(), crate::human_size(body.len())),
            (
                "Complete".into(),
                if response.truncated {
                    "no, cut by the host"
                } else {
                    "yes"
                }
                .into(),
            ),
            ("Final URL".into(), response.url.clone()),
        ],
    });
    blocks.push(Block::Table {
        columns: vec!["Header".into(), "Value".into()],
        rows: response
            .headers
            .iter()
            .map(|(name, value)| vec![name.clone(), value.clone()])
            .collect(),
    });
    if stats {
        blocks.push(Block::Heading {
            text: "Body".into(),
        });
        blocks.push(Block::KeyValue {
            rows: body_stats(&String::from_utf8_lossy(&body)),
        });
    }
    let button = if stats {
        ("hide-stats", "Hide body statistics")
    } else {
        ("body-stats", "Show body statistics")
    };
    blocks.push(Block::Actions {
        actions: vec![ActionButton {
            id: button.0.into(),
            label: button.1.into(),
        }],
    });
    Ok(reply(blocks, stats))
}

fn reply(blocks: Vec<Block>, stats: bool) -> ViewReply {
    ViewReply {
        view: View { blocks },
        state: Some(json!({ "stats": stats })),
        update_request: None,
    }
}

fn body_stats(text: &str) -> Vec<(String, String)> {
    let json = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(map)) => format!("object with {} keys", map.len()),
        Ok(serde_json::Value::Array(items)) => format!("array with {} items", items.len()),
        Ok(_) => "a single value".into(),
        Err(_) => "no".into(),
    };
    vec![
        ("Lines".into(), text.lines().count().to_string()),
        ("Words".into(), text.split_whitespace().count().to_string()),
        ("Characters".into(), text.chars().count().to_string()),
        ("JSON".into(), json),
    ]
}
