// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The request tab `query`. The request URL is the truth: every render reads the form values
//! from it; the tab state keeps only what the URL cannot hold (a form that did not apply, the
//! filter builder's rows, the version chosen without metadata).
//!
//! Actions: `apply` (the main form), `build` (the filter builder form), `add-row`,
//! `remove-row`.

use rt_plugin_sdk::api::{
    ActionButton, Block, FieldKind, FormField, PluginContext, View, ViewReply,
};
use rt_plugin_sdk::model::RequestSpec;
use rt_plugin_sdk::{Action, NoteLevel, Result, Tab, bail};
use serde::{Deserialize, Serialize};

use super::literal::{self, Condition, Operator};
use super::url::{query_of, update_spec};
use super::{Query, parse_list, parse_orderby, slot};
use crate::edm::{EntityType, Model, Property, Version};
use crate::{metadata, service};

/// Rows of the filter builder: default and limits.
const ROWS: usize = 3;
const MAX_ROWS: usize = 10;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct State {
    /// The request URL that `draft` belongs to.
    #[serde(default)]
    url: String,
    /// Values of the main form that could not be applied, by field name.
    #[serde(default)]
    draft: Vec<(String, String)>,
    /// Filter builder: number of rows, rows as `(property, operator, value)`, `and`/`or`.
    #[serde(default)]
    rows: Option<usize>,
    #[serde(default)]
    builder: Vec<(String, String, String)>,
    #[serde(default)]
    join: String,
    /// Chosen in the plain form (no metadata).
    #[serde(default)]
    version: Option<Version>,
}

impl State {
    fn from(value: Option<&serde_json::Value>) -> State {
        value
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    fn rows(&self) -> usize {
        self.rows.unwrap_or(ROWS).clamp(1, MAX_ROWS)
    }
}

/// What the request URL points to.
struct Scope {
    root: Option<String>,
    model: Option<Model>,
    /// The first resource segment without key predicate, when it is an entity set.
    set: Option<String>,
    /// The entity type at the end of the path (navigation followed).
    entity: Option<EntityType>,
    /// Why there is no entity type although there is a model.
    problem: Option<String>,
    version: Version,
    version_source: &'static str,
}

pub fn render(tab: Tab) -> Result<ViewReply> {
    let mut state = State::from(tab.state.as_ref());
    if state.url != tab.spec.url {
        state.draft.clear();
    }
    let scope = scope(&tab.spec, tab.context.as_ref(), &state);
    Ok(view(&tab.spec, &scope, state, None, None))
}

pub fn action(action: Action) -> Result<ViewReply> {
    let mut state = State::from(action.state.as_ref());
    let spec = &action.spec;
    match action.action.as_str() {
        "apply" => {
            if let Some(v) = action.input("version") {
                state.version = parse_version(v).or(state.version);
            }
            let scope = scope(spec, action.context.as_ref(), &state);
            match query_from_form(&action, &scope) {
                Ok(query) => {
                    let updated = update_spec(spec, &query, scope.version);
                    state.draft.clear();
                    state.url = updated.url.clone();
                    let notice = info("Applied to the request URL.");
                    Ok(view(
                        &updated,
                        &scope,
                        state,
                        Some(notice),
                        Some(updated.clone()),
                    ))
                }
                Err(e) => {
                    state.draft = action.input.clone();
                    state.url = spec.url.clone();
                    Ok(view(spec, &scope, state, Some(warn(e)), None))
                }
            }
        }
        "build" => {
            let scope = scope(spec, action.context.as_ref(), &state);
            let rows = state.rows();
            state.builder = (0..rows)
                .map(|i| {
                    let get = |f: &str| action.input(&format!("{f}.{i}")).unwrap_or("").to_owned();
                    (get("prop"), get("op"), get("value"))
                })
                .collect();
            state.join = action.input("join").unwrap_or("and").to_owned();
            match filter_from_builder(&state, &scope) {
                Ok(expression) => {
                    let mut query = Query::parse(query_of(&spec.url));
                    query.other.retain(|(n, _)| slot(n) != "$filter");
                    query.filter = Some(expression.clone());
                    let updated = update_spec(spec, &query, scope.version);
                    state.draft.clear();
                    state.url = updated.url.clone();
                    let notice = info(format!("$filter set to: {expression}"));
                    Ok(view(
                        &updated,
                        &scope,
                        state,
                        Some(notice),
                        Some(updated.clone()),
                    ))
                }
                Err(e) => Ok(view(spec, &scope, state, Some(warn(e)), None)),
            }
        }
        "add-row" | "remove-row" => {
            let rows = state.rows();
            state.rows = Some(if action.action == "add-row" {
                (rows + 1).min(MAX_ROWS)
            } else {
                rows.saturating_sub(1).max(1)
            });
            if state.url != spec.url {
                state.draft.clear();
            }
            let scope = scope(spec, action.context.as_ref(), &state);
            Ok(view(spec, &scope, state, None, None))
        }
        other => bail!("unknown action: {other}"),
    }
}

// -------------------------------------------------------------------------------------------
// Where the request goes.

fn scope(spec: &RequestSpec, context: Option<&PluginContext>, state: &State) -> Scope {
    let variables = context.map(|c| c.variables.as_slice()).unwrap_or_default();
    let url = super::encode::resolve(&spec.url, variables);
    let roots = known_roots();
    let target = if url.contains("://") {
        service::split(&url, &roots)
    } else {
        None
    };
    let root = target.as_ref().map(|t| t.service_root.clone());
    let model = root.as_deref().and_then(cached);
    let (set, entity, problem) = match (&model, &target) {
        (Some(m), Some(t)) => resolve_entity(m, &t.resource),
        _ => (None, None, None),
    };
    let query = Query::parse(query_of(&spec.url));
    let (version, version_source) = if let Some(m) = &model {
        (m.version, "from $metadata")
    } else if let Some(v) = state.version {
        (v, "chosen")
    } else if let Some(v) = target.as_ref().and_then(|t| t.version) {
        (v, "guessed from the URL")
    } else if query
        .other
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case("$inlinecount"))
        || query_of(&spec.url).contains("$inlinecount")
    {
        (Version::V2, "guessed from $inlinecount")
    } else {
        (Version::V4, "assumed")
    };
    Scope {
        root,
        model,
        set,
        entity,
        problem,
        version,
        version_source,
    }
}

/// Service roots with cached metadata. Reads through `metadata::known_roots` and, as long as
/// that is the interface stub, the fixed storage format (`metadata-roots`, a JSON array).
fn known_roots() -> Vec<String> {
    let mut roots = metadata::known_roots().unwrap_or_default();
    if let Ok(Some(value)) = rt_plugin_sdk::host::storage_get("metadata-roots") {
        for root in serde_json::from_value::<Vec<String>>(value).unwrap_or_default() {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// The cached model of `root`: `metadata::cached`, or the fixed storage format
/// (`service::storage_key("metadata", root)`, a serialized [`Model`]).
fn cached(root: &str) -> Option<Model> {
    metadata::cached(root).ok().flatten().or_else(|| {
        rt_plugin_sdk::host::storage_get(service::storage_key("metadata", root))
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok())
    })
}

fn strip_key(segment: &str) -> &str {
    segment.split('(').next().unwrap_or(segment)
}

/// Entity set of the first segment (key predicate stripped) and the entity type after
/// following navigation segments (`SalesOrderSet('1')/ToItems`).
fn resolve_entity(
    model: &Model,
    resource: &str,
) -> (Option<String>, Option<EntityType>, Option<String>) {
    let segments: Vec<&str> = resource.split('/').filter(|s| !s.is_empty()).collect();
    let Some(first) = segments.first() else {
        let sets: Vec<&str> = model.entity_sets.iter().map(|s| s.name.as_str()).collect();
        return (
            None,
            None,
            Some(format!(
                "The URL points to the service root. Add an entity set to the path: {}.",
                sets.join(", ")
            )),
        );
    };
    let name = strip_key(first);
    let Some(set) = model.entity_set(name) else {
        return (
            None,
            None,
            Some(format!("{name} is not an entity set of this service.")),
        );
    };
    let mut ty = model.entity_type(&set.entity_type);
    for segment in &segments[1..] {
        let Some(current) = ty else { break };
        let Some(nav) = current
            .navigation
            .iter()
            .find(|n| n.name == strip_key(segment))
        else {
            break;
        };
        let target = nav
            .target
            .strip_prefix("Collection(")
            .and_then(|t| t.strip_suffix(')'))
            .unwrap_or(&nav.target);
        ty = model.entity_type(target);
    }
    let problem = ty
        .is_none()
        .then(|| format!("The entity type {} is not in $metadata.", set.entity_type));
    (Some(set.name.clone()), ty.cloned(), problem)
}

fn parse_version(v: &str) -> Option<Version> {
    match v.trim().to_ascii_uppercase().as_str() {
        "V2" => Some(Version::V2),
        "V4" => Some(Version::V4),
        _ => None,
    }
}

fn version_label(v: Version) -> &'static str {
    match v {
        Version::V2 => "V2",
        Version::V4 => "V4",
    }
}

fn says_false(p: &Property, term: &str) -> bool {
    p.annotations
        .iter()
        .any(|(k, v)| k.to_ascii_lowercase().ends_with(term) && v.eq_ignore_ascii_case("false"))
}

/// Primitive (or V4 enum) properties: not collections, not complex types.
fn primitive(model: Option<&Model>, p: &Property) -> bool {
    !p.r#type.starts_with("Collection(")
        && (p.r#type.starts_with("Edm.")
            || model.is_none_or(|m| !m.complex_types.iter().any(|c| c.name == p.r#type)))
}

fn filterable<'a>(scope: &'a Scope, entity: &'a EntityType) -> Vec<&'a Property> {
    entity
        .properties
        .iter()
        .filter(|p| primitive(scope.model.as_ref(), p) && !says_false(p, "filterable"))
        .collect()
}

fn sortable<'a>(scope: &'a Scope, entity: &'a EntityType) -> Vec<&'a Property> {
    entity
        .properties
        .iter()
        .filter(|p| primitive(scope.model.as_ref(), p) && !says_false(p, "sortable"))
        .collect()
}

// -------------------------------------------------------------------------------------------
// Form → query.

fn query_from_form(action: &Action, scope: &Scope) -> std::result::Result<Query, String> {
    let input = |name: &str| action.input(name).unwrap_or("").trim().to_owned();
    let checked = |name: &str| action.input(name) == Some("true");
    let mut q = Query::parse(query_of(&action.spec.url));
    // The form owns these options: duplicates or unparsable ones in `other` go.
    q.other.retain(|(n, _)| {
        !matches!(
            slot(n).as_str(),
            "$filter" | "$select" | "$expand" | "$orderby" | "$top" | "$skip"
        )
    });

    let filter = input("filter");
    q.filter = (!filter.is_empty()).then_some(filter);
    q.expand = parse_list(&input("expand"));
    if let Some(entity) = &scope.entity {
        q.select = entity
            .properties
            .iter()
            .filter(|p| checked(&format!("select.{}", p.name)))
            .map(|p| p.name.clone())
            .collect();
        q.select.extend(parse_list(&input("select_more")));
        let first = input("orderby");
        q.orderby = if first.is_empty() {
            Vec::new()
        } else {
            vec![(first, checked("orderby_desc"))]
        };
        q.orderby.extend(parse_orderby(&input("orderby_more")));
    } else {
        q.select = parse_list(&input("select"));
        q.orderby = parse_orderby(&input("orderby"));
    }
    q.top = None;
    q.skip = None;
    for (name, field) in [("$top", "top"), ("$skip", "skip")] {
        let value = input(field);
        if value.is_empty() {
            continue;
        }
        if let Ok(n) = value.parse::<u64>() {
            if field == "top" {
                q.top = Some(n);
            } else {
                q.skip = Some(n);
            }
        } else if !super::encode::template_ranges(&value).is_empty() {
            q.other
                .push((name.to_owned(), super::encode::escape_raw(&value, &[])));
        } else {
            return Err(format!(
                "{name} must be a whole number or a {{{{variable}}}}, not '{value}'."
            ));
        }
    }
    let count = checked("count");
    if count {
        q.other.retain(|(n, _)| slot(n) != "$count");
    }
    q.count = count;
    Ok(q)
}

fn filter_from_builder(state: &State, scope: &Scope) -> std::result::Result<String, String> {
    let props = scope
        .entity
        .as_ref()
        .map(|e| e.properties.as_slice())
        .unwrap_or_default();
    let mut conditions = Vec::new();
    for (property, op, value) in &state.builder {
        let property = property.trim();
        if property.is_empty() {
            continue;
        }
        let operator = Operator::parse(op).unwrap_or(Operator::Eq);
        let edm_type = props
            .iter()
            .find(|p| p.name == property)
            .map_or("Edm.String", |p| p.r#type.as_str());
        conditions.push(Condition {
            property: property.to_owned(),
            edm_type: edm_type.to_owned(),
            operator,
            value: value.clone(),
        });
    }
    if conditions.is_empty() {
        return Err("Pick a property in at least one row.".into());
    }
    literal::filter(&conditions, state.join == "or", scope.version)
}

// -------------------------------------------------------------------------------------------
// The view.

fn info(text: impl Into<String>) -> Block {
    Block::Notice {
        level: NoteLevel::Info,
        text: text.into(),
    }
}

fn warn(text: impl Into<String>) -> Block {
    Block::Notice {
        level: NoteLevel::Warning,
        text: text.into(),
    }
}

fn text_field(name: &str, label: &str, value: String) -> FormField {
    FormField {
        name: name.into(),
        label: label.into(),
        kind: FieldKind::Text,
        value,
        options: Vec::new(),
    }
}

fn checkbox(name: &str, label: &str, on: bool) -> FormField {
    FormField {
        name: name.into(),
        label: label.into(),
        kind: FieldKind::Checkbox,
        value: on.to_string(),
        options: Vec::new(),
    }
}

fn select(name: &str, label: &str, value: String, options: Vec<String>) -> FormField {
    FormField {
        name: name.into(),
        label: label.into(),
        kind: FieldKind::Select,
        value,
        options,
    }
}

fn view(
    spec: &RequestSpec,
    scope: &Scope,
    state: State,
    message: Option<Block>,
    update_request: Option<RequestSpec>,
) -> ViewReply {
    let mut blocks = vec![Block::Heading {
        text: "Query builder".into(),
    }];
    blocks.extend(message);

    let mut rows = vec![(
        "Service root".to_owned(),
        scope.root.clone().unwrap_or_else(|| "unknown".into()),
    )];
    if let Some(set) = &scope.set {
        rows.push(("Entity set".into(), set.clone()));
    }
    if let Some(entity) = &scope.entity {
        rows.push(("Entity type".into(), entity.name.clone()));
    }
    rows.push((
        "OData version".into(),
        format!(
            "{} ({})",
            version_label(scope.version),
            scope.version_source
        ),
    ));
    blocks.push(Block::KeyValue { rows });

    match (&scope.root, &scope.model) {
        (None, _) => blocks.push(warn(
            "The request URL has no scheme and host, so the service root is unknown (a \
             {{variable}} without a value in the selected environment?). The fields below are \
             plain text.",
        )),
        (Some(root), None) => blocks.push(info(format!(
            "No metadata cached for this service. Send a request to {root}$metadata first; \
             until then the fields below are plain text."
        ))),
        (Some(_), Some(_)) => blocks.extend(scope.problem.clone().map(info)),
    }

    let query = Query::parse(query_of(&spec.url));
    let draft = |name: &str| {
        state
            .draft
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    let value = |name: &str, default: String| draft(name).unwrap_or(default);
    let on = |name: &str, default: bool| draft(name).map_or(default, |v| v == "true");
    let raw_other = |s: &str| {
        query
            .other
            .iter()
            .find(|(n, _)| slot(n) == s)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let top = query
        .top
        .map_or_else(|| raw_other("$top"), |n| n.to_string());
    let skip = query
        .skip
        .map_or_else(|| raw_other("$skip"), |n| n.to_string());
    let count_label = match scope.version {
        Version::V2 => "Count ($inlinecount=allpages)",
        Version::V4 => "Count ($count=true)",
    };

    let mut fields = vec![text_field(
        "filter",
        "$filter",
        value("filter", query.filter.clone().unwrap_or_default()),
    )];
    if let Some(entity) = &scope.entity {
        let filterable = filterable(scope, entity);
        if !filterable.is_empty() {
            blocks.push(Block::Text {
                text: "Filterable properties, with examples for their types:".into(),
            });
            blocks.push(Block::Table {
                columns: vec!["Property".into(), "Type".into(), "Example".into()],
                rows: filterable
                    .iter()
                    .map(|p| {
                        vec![
                            p.name.clone(),
                            p.r#type.clone(),
                            literal::example(&p.name, &p.r#type, scope.version),
                        ]
                    })
                    .collect(),
            });
        }
        for p in &entity.properties {
            let label = if entity.key.contains(&p.name) {
                format!("$select {} (key)", p.name)
            } else {
                format!("$select {}", p.name)
            };
            let name = format!("select.{}", p.name);
            fields.push(checkbox(
                &name,
                &label,
                on(&name, query.select.contains(&p.name)),
            ));
        }
        let more: Vec<&str> = query
            .select
            .iter()
            .filter(|s| !entity.properties.iter().any(|p| &p.name == *s))
            .map(String::as_str)
            .collect();
        fields.push(text_field(
            "select_more",
            "$select, more (paths, *)",
            value("select_more", more.join(",")),
        ));
        let navigation: Vec<String> = entity
            .navigation
            .iter()
            .map(|n| {
                if n.many {
                    format!("{} (many)", n.name)
                } else {
                    n.name.clone()
                }
            })
            .collect();
        let expand_label = if navigation.is_empty() {
            "$expand (no navigation properties)".to_owned()
        } else {
            format!("$expand ({})", navigation.join(", "))
        };
        fields.push(text_field(
            "expand",
            &expand_label,
            value("expand", query.expand.join(",")),
        ));
        let sortable: Vec<String> = sortable(scope, entity)
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let (first, rest) = match query.orderby.split_first() {
            Some((f, rest)) if sortable.contains(&f.0) => (Some(f.clone()), rest),
            _ => (None, query.orderby.as_slice()),
        };
        let mut options = vec![String::new()];
        options.extend(sortable);
        fields.push(select(
            "orderby",
            "$orderby",
            value(
                "orderby",
                first.as_ref().map(|f| f.0.clone()).unwrap_or_default(),
            ),
            options,
        ));
        fields.push(checkbox(
            "orderby_desc",
            "descending",
            on("orderby_desc", first.is_some_and(|f| f.1)),
        ));
        fields.push(text_field(
            "orderby_more",
            "then by (e.g. Name desc,ID)",
            value("orderby_more", orderby_text(rest)),
        ));
    } else {
        fields.push(text_field(
            "select",
            "$select (comma-separated)",
            value("select", query.select.join(",")),
        ));
        fields.push(text_field(
            "expand",
            "$expand (comma-separated)",
            value("expand", query.expand.join(",")),
        ));
        fields.push(text_field(
            "orderby",
            "$orderby (e.g. Name desc,ID)",
            value("orderby", orderby_text(&query.orderby)),
        ));
    }
    fields.push(text_field("top", "$top", value("top", top)));
    fields.push(text_field("skip", "$skip", value("skip", skip)));
    fields.push(checkbox("count", count_label, on("count", query.count)));
    if scope.model.is_none() {
        fields.push(select(
            "version",
            "OData version",
            value("version", version_label(scope.version).into()),
            vec!["V4".into(), "V2".into()],
        ));
    }
    blocks.push(Block::Form {
        id: "apply".into(),
        fields,
        submit: "Apply to URL".into(),
    });
    blocks.push(Block::Heading { text: "URL".into() });
    blocks.push(Block::Code {
        language: None,
        text: spec.url.clone(),
    });

    if let Some(entity) = &scope.entity {
        builder(&mut blocks, scope, entity, &state);
    }

    ViewReply {
        view: View { blocks },
        state: Some(serde_json::to_value(&state).unwrap_or_default()),
        update_request,
        create_requests: Vec::new(),
        add_environments: Vec::new(),
    }
}

fn orderby_text(items: &[(String, bool)]) -> String {
    items
        .iter()
        .map(|(p, desc)| {
            if *desc {
                format!("{p} desc")
            } else {
                p.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn builder(blocks: &mut Vec<Block>, scope: &Scope, entity: &EntityType, state: &State) {
    blocks.push(Block::Heading {
        text: "Filter builder".into(),
    });
    blocks.push(Block::Text {
        text: format!(
            "One condition per row, written as OData {} literals for the property's type. Rows \
             without a property are skipped; null is the null literal. The result replaces \
             $filter.",
            version_label(scope.version)
        ),
    });
    let mut properties = vec![String::new()];
    properties.extend(filterable(scope, entity).iter().map(|p| p.name.clone()));
    let operators: Vec<String> = Operator::ALL
        .iter()
        .map(|o| o.as_str().to_owned())
        .collect();
    let mut fields = Vec::new();
    for i in 0..state.rows() {
        let (p, o, v) = state.builder.get(i).cloned().unwrap_or_default();
        let n = i + 1;
        fields.push(select(
            &format!("prop.{i}"),
            &format!("Property {n}"),
            p,
            properties.clone(),
        ));
        fields.push(select(
            &format!("op.{i}"),
            &format!("Operator {n}"),
            if o.is_empty() { "eq".into() } else { o },
            operators.clone(),
        ));
        fields.push(text_field(&format!("value.{i}"), &format!("Value {n}"), v));
    }
    fields.push(select(
        "join",
        "Combine rows with",
        if state.join == "or" { "or" } else { "and" }.into(),
        vec!["and".into(), "or".into()],
    ));
    blocks.push(Block::Form {
        id: "build".into(),
        fields,
        submit: "Set $filter".into(),
    });
    let mut actions = Vec::new();
    if state.rows() < MAX_ROWS {
        actions.push(ActionButton {
            id: "add-row".into(),
            label: "Add a row".into(),
        });
    }
    if state.rows() > 1 {
        actions.push(ActionButton {
            id: "remove-row".into(),
            label: "Remove a row".into(),
        });
    }
    blocks.push(Block::Actions { actions });
}
