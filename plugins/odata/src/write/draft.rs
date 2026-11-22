// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Request tab `draft`: draft actions of RAP (`Edit`, `Prepare`, `Activate`, `Discard`) and
//! CAP (`draftEdit`, `draftPrepare`, `draftActivate`; discard is `DELETE` on the draft) for
//! the entity of the current request. Each button rewrites the request (`update_request`);
//! sending it is up to the user, and `prepare` adds the CSRF token and `If-Match`.
//!
//! Needs the cached model of the service (`Common.DraftRoot` → `EntitySet.draft`, the key
//! properties and the namespace of the actions).

use rt_plugin_sdk::api::{ActionButton, Block, View, ViewReply};
use rt_plugin_sdk::model::{Body, RawKind, RequestSpec};
use rt_plugin_sdk::{Action, NoteLevel, Result, Tab};

use super::util::{self, TemplateTarget};
use crate::edm::Model;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Rap,
    Cap,
}

/// The draft style of a model: CAP if it has `draftEdit`, RAP if it has a bound `Edit`,
/// otherwise by the URL (`/sap/opu/odata4/` is RAP).
pub fn style(model: &Model) -> Style {
    let has = |name: &str| {
        model
            .operations
            .iter()
            .any(|o| o.name.rsplit('.').next() == Some(name))
    };
    if has("draftEdit") {
        Style::Cap
    } else if has("Edit") || model.service_root.contains("/sap/opu/odata4/") {
        Style::Rap
    } else {
        Style::Cap
    }
}

/// The qualified name of a draft action (`Edit`/`Prepare`/`Activate`/`Discard`).
fn action_name(model: &Model, style: Style, which: &str) -> String {
    let simple = match style {
        Style::Rap => which.to_owned(),
        Style::Cap => format!("draft{which}"),
    };
    model
        .operations
        .iter()
        .find(|o| o.name.rsplit('.').next() == Some(simple.as_str()) && o.name.contains('.'))
        .map_or_else(
            || format!("{}.{simple}", model.namespace),
            |o| o.name.clone(),
        )
}

/// The entity of the current request: entity set, key pairs and whether it is the active
/// instance.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entity {
    set: String,
    /// `(name, literal)` in key order, without `IsActiveEntity`.
    key: Vec<(String, String)>,
    active: bool,
}

impl Entity {
    fn segment(&self, active: bool) -> String {
        let mut parts: Vec<String> = self.key.iter().map(|(k, v)| format!("{k}={v}")).collect();
        parts.push(format!("IsActiveEntity={active}"));
        format!("{}({})", self.set, parts.join(","))
    }
}

struct Context {
    target: TemplateTarget,
    model: Model,
    draft: bool,
    entity: Option<Entity>,
    set: String,
}

fn context(spec: &RequestSpec, vars: &[(String, String)]) -> std::result::Result<Context, String> {
    let target =
        util::template_target(&spec.url, vars).ok_or("The request URL is not an OData URL.")?;
    let root = target.resolved_root.clone().ok_or_else(|| {
        format!(
            "The service root {} cannot be resolved; select the environment that defines its variables.",
            target.root
        )
    })?;
    let model = util::model(&root).ok_or_else(|| {
        format!("No $metadata cached for {root}: send GET {root}$metadata once (the OData plugin keeps it).")
    })?;
    let first = target
        .resource
        .split('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    let (set_name, predicate) = util::split_segment(&first);
    let set = model
        .entity_set(set_name)
        .ok_or_else(|| format!("{set_name} is not an entity set of this service."))?
        .clone();
    let ty = model
        .entity_type(&set.entity_type)
        .ok_or_else(|| format!("The type of {set_name} is missing in the model."))?
        .clone();
    // After a draft action the request URL still names it: the buttons follow its result
    // (after Edit there is a draft, after Activate or Discard the active instance).
    let after_action = target
        .resource
        .split('/')
        .nth(1)
        .and_then(|a| a.rsplit('.').next())
        .and_then(|a| match a {
            "Edit" | "draftEdit" | "Prepare" | "draftPrepare" => Some(false),
            "Activate" | "draftActivate" | "Discard" => Some(true),
            _ => None,
        });
    let entity = predicate.map(|p| {
        let mut key = Vec::new();
        let mut active = true;
        let names: Vec<&String> = ty.key.iter().filter(|k| *k != "IsActiveEntity").collect();
        for (i, part) in util::split_key(p).into_iter().enumerate() {
            let (name, value) = match part.split_once('=') {
                Some((n, v)) => (n.trim().to_owned(), v.trim().to_owned()),
                None => (
                    names.get(i).map_or_else(String::new, |n| (*n).clone()),
                    part.trim().to_owned(),
                ),
            };
            if name == "IsActiveEntity" {
                active = value != "false";
            } else {
                key.push((name, value));
            }
        }
        Entity {
            set: set.name.clone(),
            key,
            active: after_action.unwrap_or(active),
        }
    });
    Ok(Context {
        target,
        draft: set.draft,
        set: set.name,
        model,
        entity,
    })
}

fn button(id: &str, label: &str) -> ActionButton {
    ActionButton {
        id: id.into(),
        label: label.into(),
    }
}

fn view(
    spec: &RequestSpec,
    vars: &[(String, String)],
    notice: Option<(NoteLevel, String)>,
) -> ViewReply {
    let mut blocks = vec![Block::Heading {
        text: "Draft".into(),
    }];
    if let Some((level, text)) = notice {
        blocks.push(Block::Notice { level, text });
    }
    match context(spec, vars) {
        Err(text) => blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text,
        }),
        Ok(c) if !c.draft => blocks.push(Block::Notice {
            level: NoteLevel::Info,
            text: format!("{} is not draft-enabled (no Common.DraftRoot).", c.set),
        }),
        Ok(c) => {
            let style = style(&c.model);
            let mut rows = vec![
                ("Entity set".into(), c.set.clone()),
                (
                    "Draft actions".into(),
                    match style {
                        Style::Rap => "RAP (Edit, Prepare, Activate, Discard)".into(),
                        Style::Cap => "CAP (draftEdit, draftPrepare, draftActivate, DELETE)".into(),
                    },
                ),
            ];
            let mut actions = Vec::new();
            match &c.entity {
                None => {
                    blocks.push(Block::Text {
                        text: "Open a request on one entity (with its key) to edit, activate or discard its draft.".into(),
                    });
                    actions.push(button("new", "Create draft (POST)"));
                }
                Some(e) => {
                    let key: Vec<String> = e.key.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    rows.push(("Key".into(), key.join(", ")));
                    rows.push((
                        "Instance".into(),
                        if e.active {
                            "active".into()
                        } else {
                            "draft".into()
                        },
                    ));
                    if e.active {
                        actions.push(button("edit", "Edit (create draft)"));
                        actions.push(button("open-draft", "Open draft"));
                    } else {
                        actions.push(button("prepare", "Prepare"));
                        actions.push(button("activate", "Activate"));
                        actions.push(button("discard", "Discard draft"));
                        actions.push(button("open-active", "Open active"));
                    }
                }
            }
            blocks.push(Block::KeyValue { rows });
            blocks.push(Block::Actions { actions });
            blocks.push(Block::Text {
                text: "Each button rewrites the request; send it to run the action. CSRF token and If-Match are added when sending.".into(),
            });
        }
    }
    ViewReply {
        view: View { blocks },
        ..ViewReply::default()
    }
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let vars = tab.context.map(|c| c.variables).unwrap_or_default();
    Ok(view(&tab.spec, &vars, None))
}

/// The rewritten request for a draft action, or why there is none.
pub fn rewrite(
    spec: &RequestSpec,
    vars: &[(String, String)],
    which: &str,
) -> std::result::Result<RequestSpec, String> {
    let c = context(spec, vars)?;
    if !c.draft {
        return Err(format!("{} is not draft-enabled.", c.set));
    }
    let style = style(&c.model);
    let root = &c.target.root;
    let query = util::sap_query(&c.target.query);
    let (method, path, body) = match (which, &c.entity) {
        ("new", _) => ("POST", c.set.clone(), Some("{}".to_owned())),
        (_, None) => return Err("The request does not address one entity.".into()),
        ("edit", Some(e)) => (
            "POST",
            format!(
                "{}/{}",
                e.segment(true),
                action_name(&c.model, style, "Edit")
            ),
            Some(r#"{"PreserveChanges":true}"#.to_owned()),
        ),
        ("prepare", Some(e)) => (
            "POST",
            format!(
                "{}/{}",
                e.segment(false),
                action_name(&c.model, style, "Prepare")
            ),
            Some("{}".to_owned()),
        ),
        ("activate", Some(e)) => (
            "POST",
            format!(
                "{}/{}",
                e.segment(false),
                action_name(&c.model, style, "Activate")
            ),
            Some("{}".to_owned()),
        ),
        ("discard", Some(e)) => match style {
            Style::Rap => (
                "POST",
                format!(
                    "{}/{}",
                    e.segment(false),
                    action_name(&c.model, style, "Discard")
                ),
                Some("{}".to_owned()),
            ),
            Style::Cap => ("DELETE", e.segment(false), None),
        },
        ("open-draft", Some(e)) => ("GET", e.segment(false), None),
        ("open-active", Some(e)) => ("GET", e.segment(true), None),
        (other, _) => return Err(format!("unknown action {other}")),
    };
    let mut new = spec.clone();
    new.method = method.into();
    new.url = format!("{root}{path}{query}");
    new.params = util::sap_params(&spec.params);
    new.headers.retain(|h| {
        !["if-match", "x-csrf-token", "x-http-method", "content-type"]
            .contains(&h.name.to_ascii_lowercase().as_str())
    });
    new.body = match body {
        Some(data) => {
            new.headers.push(rt_plugin_sdk::model::KeyValue::new(
                "Content-Type",
                "application/json",
            ));
            Body::Raw {
                kind: RawKind::Json,
                data,
            }
        }
        None => Body::None,
    };
    Ok(new)
}

pub fn action(action: Action) -> Result<ViewReply> {
    let vars = action
        .context
        .as_ref()
        .map(|c| c.variables.clone())
        .unwrap_or_default();
    match rewrite(&action.spec, &vars, &action.action) {
        Ok(new) => {
            let text = format!(
                "Request rewritten: {} {}. Send it to run the action.",
                new.method, new.url
            );
            let mut reply = view(&new, &vars, Some((NoteLevel::Info, text)));
            reply.update_request = Some(new);
            Ok(reply)
        }
        Err(text) => Ok(view(&action.spec, &vars, Some((NoteLevel::Warning, text)))),
    }
}
