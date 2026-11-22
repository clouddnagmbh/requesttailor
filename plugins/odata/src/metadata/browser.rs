// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The entity browser, collection tab `entities`: the cached services as a tree (entity sets,
//! their properties and navigation, operations), "Create requests" and "Forget metadata" per
//! service, and a form to load another service's `$metadata`.
//!
//! Loading from the tab needs the `http` host call outside a send. The manifest asks only for
//! `{request}`, which covers the host of the request a hook runs for, and a tab runs for none,
//! so the host refuses it (see `API-WUENSCHE.md`). The tab then explains the way that always
//! works, sending `GET <root>$metadata` (the `response` hook caches it), and offers to add that
//! request to the collection.
//!
//! Action ids: `load` (the form), `create-requests <root>`, `forget <root>`,
//! `add-metadata-request <root>`. State: `{ "root": "<last root entered>" }`.

use rt_plugin_sdk::api::{
    ActionButton, Block, FieldKind, FormField, NewRequest, PluginContext, PluginEnvironment,
    PluginRequest, PluginVariable, TreeItem, View, ViewReply,
};
use rt_plugin_sdk::model::{KeyValue, RequestSpec};
use rt_plugin_sdk::{Action, Error, NoteLevel, Result, Tab, host};

use crate::edm::{EntitySet, Model, Version};

use super::{cache, parse, requests_for};

/// The environment that gets `baseUrl` when none is selected.
pub const DEFAULT_ENVIRONMENT: &str = "Default";

pub fn render(tab: Tab) -> Result<ViewReply> {
    let root = state_root(tab.state.as_ref())
        .or_else(|| base_url(tab.context.as_ref()))
        .unwrap_or_default();
    view(None, Vec::new(), &root)
}

pub fn action(action: Action) -> Result<ViewReply> {
    let previous = state_root(action.state.as_ref()).unwrap_or_default();
    let (id, arg) = action
        .action
        .split_once(' ')
        .unwrap_or((action.action.as_str(), ""));
    match id {
        "load" => {
            let input = action.input("root").unwrap_or_default();
            let Some(root) = normalize_root(input) else {
                return view(
                    Some((
                        NoteLevel::Error,
                        "Enter the service root URL, starting with http:// or https://.".into(),
                    )),
                    Vec::new(),
                    input,
                );
            };
            load(&root)
        }
        "create-requests" => {
            let Some(model) = cache::cached(arg)? else {
                return view(
                    Some((NoteLevel::Warning, format!("No metadata cached for {arg}."))),
                    Vec::new(),
                    &previous,
                );
            };
            let requests = requests_for(&model);
            let environment = action
                .context
                .as_ref()
                .and_then(|c| c.environment.clone())
                .unwrap_or_else(|| DEFAULT_ENVIRONMENT.to_owned());
            let mut reply = view(
                Some((
                    NoteLevel::Info,
                    format!(
                        "{} requests for {} added. They start with {{{{baseUrl}}}}; environment \
                         {environment} gets baseUrl = {} unless it has one.",
                        requests.len(),
                        model.namespace,
                        model.service_root
                    ),
                )),
                Vec::new(),
                &previous,
            )?;
            reply.create_requests = requests;
            reply.add_environments = vec![PluginEnvironment {
                name: environment,
                variables: vec![PluginVariable {
                    name: "baseUrl".into(),
                    value: model.service_root.clone(),
                    secret: false,
                }],
            }];
            Ok(reply)
        }
        "forget" => {
            cache::forget(arg)?;
            view(
                Some((NoteLevel::Info, format!("Metadata of {arg} forgotten."))),
                Vec::new(),
                &previous,
            )
        }
        "add-metadata-request" => {
            let mut reply = view(
                Some((
                    NoteLevel::Info,
                    format!(
                        "Request \"$metadata\" added for {arg}. Send it, and the service shows \
                         up here."
                    ),
                )),
                Vec::new(),
                arg,
            )?;
            reply.create_requests = vec![metadata_request(arg)];
            Ok(reply)
        }
        other => Err(Error::new(format!("unknown action {other}"))),
    }
}

/// `GET <root>$metadata` as a request in the collection root.
pub fn metadata_request(root: &str) -> NewRequest {
    let spec = RequestSpec {
        name: "$metadata".into(),
        method: "GET".into(),
        url: format!("{root}$metadata"),
        headers: vec![KeyValue::new("Accept", "application/xml")],
        ..RequestSpec::default()
    };
    NewRequest {
        folder: String::new(),
        name: spec.name.clone(),
        spec,
    }
}

/// Fetches and caches `<root>$metadata`; explains the alternative when the host refuses.
fn load(root: &str) -> Result<ViewReply> {
    let url = format!("{root}$metadata");
    let request = PluginRequest {
        method: "GET".into(),
        url: url.clone(),
        headers: vec![("Accept".into(), "application/xml".into())],
        body: None,
    };
    let offer = vec![ActionButton {
        id: format!("add-metadata-request {root}"),
        label: "Add a $metadata request".into(),
    }];
    let failed = |text: String| view(Some((NoteLevel::Warning, text)), offer.clone(), root);
    let response = match host::http(request) {
        Ok(r) => r,
        Err(e) if e.message().contains("permission denied") => {
            return failed(format!(
                "This tab cannot fetch {url}: the OData plugin may only contact the host of a \
                 request that is being sent, and a collection tab sends none. Send GET {url} \
                 instead; the plugin caches every $metadata response, and the service shows up \
                 here."
            ));
        }
        Err(e) => return failed(format!("Fetching {url} failed: {e}")),
    };
    if response.status != 200 {
        return failed(format!(
            "{url} answered HTTP {}. If the service needs a login, send GET {url} as a request \
             of the collection (with its auth); the plugin caches the response.",
            response.status
        ));
    }
    let text = String::from_utf8(rt_plugin_sdk::body_bytes(&response.body)?)?;
    let model = match parse(&text, root) {
        Ok(m) => m,
        Err(e) => return failed(format!("{url} is not readable OData metadata: {e}")),
    };
    cache::store(&model)?;
    view(
        Some((NoteLevel::Info, cached_text(&model))),
        Vec::new(),
        root,
    )
}

/// "OData V2 metadata cached: N entity sets".
pub fn cached_text(model: &Model) -> String {
    format!(
        "OData {} metadata cached: {} entity sets",
        version_label(model.version),
        model.entity_sets.len()
    )
}

fn version_label(v: Version) -> &'static str {
    match v {
        Version::V2 => "V2",
        Version::V4 => "V4",
    }
}

/// The tab's view: an optional notice, the services (or how to get one), the load form.
fn view(
    notice: Option<(NoteLevel, String)>,
    offer: Vec<ActionButton>,
    root: &str,
) -> Result<ViewReply> {
    let mut blocks = vec![Block::Heading {
        text: "OData services".into(),
    }];
    if let Some((level, text)) = notice {
        blocks.push(Block::Notice { level, text });
    }
    if !offer.is_empty() {
        blocks.push(Block::Actions { actions: offer });
    }
    let models: Vec<Model> = cache::known_roots()?
        .iter()
        .filter_map(|r| cache::cached(r).ok().flatten())
        .collect();
    if models.is_empty() {
        blocks.push(Block::Text {
            text: "No OData metadata is cached yet. Load the $metadata of a service to browse \
                   its entity sets and to create requests from it. Sending any request to \
                   <service root>$metadata caches it too."
                .into(),
        });
    }
    for model in &models {
        blocks.extend(service_blocks(model));
    }
    blocks.push(Block::Form {
        id: "load".into(),
        fields: vec![FormField {
            name: "root".into(),
            label: "Service root URL".into(),
            kind: FieldKind::Text,
            value: root.to_owned(),
            options: Vec::new(),
        }],
        submit: "Load $metadata".into(),
    });
    Ok(ViewReply {
        view: View { blocks },
        state: (!root.is_empty()).then(|| serde_json::json!({ "root": root })),
        ..ViewReply::default()
    })
}

fn service_blocks(model: &Model) -> Vec<Block> {
    let root = &model.service_root;
    let mut nodes: Vec<TreeItem> = model
        .entity_sets
        .iter()
        .map(|s| set_item(model, s))
        .collect();
    if !model.operations.is_empty() {
        nodes.push(TreeItem {
            label: "Operations".into(),
            detail: Some(model.operations.len().to_string()),
            children: model
                .operations
                .iter()
                .map(|o| TreeItem {
                    label: o.name.clone(),
                    detail: Some(
                        [
                            Some(format!("{:?}", o.kind).to_lowercase()),
                            o.bound_to
                                .as_ref()
                                .map(|b| format!("bound to {}", short(b))),
                            o.returns.as_ref().map(|r| format!("returns {}", short(r))),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · "),
                    ),
                    children: o.parameters.iter().map(property_item_plain).collect(),
                    action: None,
                })
                .collect(),
            action: None,
        });
    }
    vec![
        Block::Heading { text: root.clone() },
        Block::KeyValue {
            rows: vec![
                ("Version".into(), version_label(model.version).into()),
                ("Namespace".into(), model.namespace.clone()),
                ("Entity sets".into(), model.entity_sets.len().to_string()),
                ("Operations".into(), model.operations.len().to_string()),
            ],
        },
        Block::Tree { nodes },
        Block::Actions {
            actions: vec![
                ActionButton {
                    id: format!("create-requests {root}"),
                    label: "Create requests".into(),
                },
                ActionButton {
                    id: format!("forget {root}"),
                    label: "Forget metadata".into(),
                },
            ],
        },
    ]
}

fn set_item(model: &Model, set: &EntitySet) -> TreeItem {
    let ty = model.entity_type(&set.entity_type);
    let mut flags = vec![short(&set.entity_type).to_owned()];
    if set.draft {
        flags.push("draft".into());
    }
    let denied: Vec<&str> = [
        (!set.insertable).then_some("create"),
        (!set.updatable).then_some("update"),
        (!set.deletable).then_some("delete"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !denied.is_empty() {
        flags.push(format!("no {}", denied.join("/")));
    }
    let children = ty
        .map(|t| {
            t.properties
                .iter()
                .map(|p| {
                    let mut item = property_item_plain(p);
                    if t.key.contains(&p.name) {
                        item.detail = Some(format!("key · {}", item.detail.unwrap_or_default()));
                    }
                    item
                })
                .chain(t.navigation.iter().map(|n| TreeItem {
                    label: n.name.clone(),
                    detail: Some(format!(
                        "→ {}{}",
                        short(&n.target),
                        if n.many { " (many)" } else { "" }
                    )),
                    children: Vec::new(),
                    action: None,
                }))
                .collect()
        })
        .unwrap_or_default();
    TreeItem {
        label: set.name.clone(),
        detail: Some(flags.join(" · ")),
        children,
        action: None,
    }
}

/// `Name`, detail `Edm.String(10) · "Label"`.
fn property_item_plain(p: &crate::edm::Property) -> TreeItem {
    let mut ty = p.r#type.clone();
    if let Some(len) = p.max_length {
        ty = format!("{ty}({len})");
    } else if let (Some(precision), Some(scale)) = (p.precision, &p.scale) {
        ty = format!("{ty}({precision},{scale})");
    }
    let mut detail = vec![ty];
    if let Some((_, label)) = p.annotations.iter().find(|(n, _)| n == "label") {
        detail.push(format!("\"{label}\""));
    }
    TreeItem {
        label: p.name.clone(),
        detail: Some(detail.join(" · ")),
        children: Vec::new(),
        action: None,
    }
}

/// `SalesOrder` of `com.example.SalesOrder`.
fn short(qualified: &str) -> &str {
    let inner = qualified
        .strip_prefix("Collection(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(qualified);
    inner.rsplit('.').next().unwrap_or(inner)
}

fn state_root(state: Option<&serde_json::Value>) -> Option<String> {
    state?
        .get("root")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// `baseUrl` of the selected environment (with the `variables` permission).
fn base_url(context: Option<&PluginContext>) -> Option<String> {
    context?
        .variables
        .iter()
        .find(|(n, _)| n == "baseUrl")
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty() && !v.contains("{{"))
}

/// A service root from what the user typed: without query and fragment, without a trailing
/// `$metadata`, with a trailing `/`. `None` unless it is an http(s) URL.
pub fn normalize_root(input: &str) -> Option<String> {
    let input = input.trim();
    let base = input.split(['?', '#']).next().unwrap_or_default();
    let base = base.strip_suffix("$metadata").unwrap_or(base);
    let rest = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))?;
    if rest.is_empty() || rest.starts_with('/') || base.contains(char::is_whitespace) {
        return None;
    }
    Some(if base.ends_with('/') {
        base.to_owned()
    } else {
        format!("{base}/")
    })
}
