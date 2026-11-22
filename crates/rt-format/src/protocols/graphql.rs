// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `type: graphql` items: `graphql.{method, url, headers, params, body: {query, variables}, auth}` (body variants like HTTP). Wave 8, strand B.
//!
//! A GraphQL item is an HTTP request whose body is [`Body::GraphQl`]: method (default `POST`),
//! URL, params, headers and auth as for HTTP, `settings` and `runtime` as for HTTP. The body
//! is `{query, variables}` or a list of variants (`title`, `selected`, `body`), read as the
//! selected one; a missing body is an empty query. The writer edits only what changed and
//! leaves the rest byte-identical.

use rt_core::model::{Auth, Body, Param, Protocol, RequestEntry, RequestSpec};

use super::Result;
use crate::read::{self, Builder};
use crate::write::{self, Empty, ListSpec};
use crate::yaml::{Document, Map, Node, Val, document_text, reparse};

const GRAPHQL_ORDER: &[&str] = &["method", "url", "params", "headers", "body", "auth"];
const BODY_ORDER: &[&str] = &["query", "variables"];

fn body(node: Option<&Node>) -> Body {
    let node = node.filter(|n| !n.is_null());
    let node = match node {
        Some(list) if list.is_seq() => read::selected(list).and_then(|v| v.get("body")),
        other => other,
    };
    Body::GraphQl {
        query: read::text(node.and_then(|b| b.get("query"))),
        variables: read::text(node.and_then(|b| b.get("variables"))),
    }
}

pub(crate) fn read(builder: &mut Builder<'_>, path: &str) -> Option<RequestEntry> {
    let root = &builder.docs.get(path)?.root;
    let info = root.get("info");
    let graphql = root.get("graphql");
    let field = |k: &str| graphql.and_then(|g| g.get(k));
    let runtime = root.get("runtime");
    let auth = field("auth");
    let spec = RequestSpec {
        name: info
            .and_then(|i| i.get("name"))
            .and_then(Node::text)
            .unwrap_or_else(|| read::file_stem(path))
            .to_owned(),
        seq: info.and_then(|i| i.get("seq")).and_then(Node::as_f64),
        method: field("method")
            .and_then(Node::text)
            .unwrap_or("POST")
            .to_owned(),
        url: read::text(field("url")),
        headers: read::key_values(field("headers")),
        params: read::params(field("params")),
        body: body(field("body")),
        auth: Auth::None,
        vars: read::variables(runtime.and_then(|r| r.get("variables"))),
        settings: read::settings(root.get("settings")),
        scripts: crate::runtime::scripts(runtime.and_then(|r| r.get("scripts"))),
        assertions: crate::runtime::assertions(runtime.and_then(|r| r.get("assertions"))),
        actions: crate::runtime::actions(runtime.and_then(|r| r.get("actions"))),
        protocol: Protocol::Http,
    };
    let spec = RequestSpec {
        auth: builder.auth(path, auth, Auth::None),
        ..spec
    };
    Some(RequestEntry {
        path: path.to_owned(),
        spec,
    })
}

fn body_val(query: &str, variables: &str) -> Val {
    Val::Map(vec![
        ("query", Val::str(query)),
        ("variables", Val::str(variables)),
    ])
}

fn graphql_val(spec: &RequestSpec) -> Result<Val> {
    let mut e = vec![
        ("method", Val::str(&spec.method)),
        ("url", Val::str(&spec.url)),
    ];
    if !spec.params.is_empty() {
        e.push((
            "params",
            Val::Seq(spec.params.iter().map(write::param_val).collect()),
        ));
    }
    if !spec.headers.is_empty() {
        e.push((
            "headers",
            Val::Seq(spec.headers.iter().map(write::kv_val).collect()),
        ));
    }
    if let Body::GraphQl { query, variables } = &spec.body {
        e.push(("body", body_val(query, variables)));
    }
    if let Some(auth) = write::auth_val(&spec.auth)? {
        e.push(("auth", auth));
    }
    Ok(Val::Map(e))
}

/// Text of a new GraphQL request file, as Bruno writes one.
pub(crate) fn new_text(spec: &RequestSpec) -> String {
    let mut info = vec![
        ("name", Val::str(&spec.name)),
        ("type", Val::str("graphql")),
    ];
    if let Some(seq) = spec.seq {
        info.push(("seq", Val::Plain(write::number(seq))));
    }
    document_text(
        &[
            ("info", Val::Map(info)),
            (
                "graphql",
                Val::Map(vec![
                    ("method", Val::str(&spec.method)),
                    ("url", Val::str(&spec.url)),
                ]),
            ),
        ],
        true,
    )
}

fn read_spec(doc: &Document, path: &str) -> Option<RequestSpec> {
    let docs = std::collections::BTreeMap::from([(path, doc)]);
    let mut builder = Builder::new(&docs);
    read(&mut builder, path).map(|r| r.spec)
}

pub(crate) fn apply(doc: &Document, path: &str, spec: &RequestSpec) -> Result<Document> {
    let old = read_spec(doc, path).ok_or("not a GraphQL request")?;
    if old == *spec {
        return Ok(doc.clone());
    }
    let mut doc = doc.clone();
    let nl = doc.newline;
    let root = doc
        .root
        .block_map_mut()
        .ok_or("the file is not a YAML mapping")?;
    write::write_info(root, &old, spec, nl);
    match root.get_mut("graphql").and_then(Node::block_map_mut) {
        None => root.put("graphql", &graphql_val(spec)?, write::ROOT_ORDER, true, nl),
        Some(graphql) => write_graphql(graphql, &old, spec, nl)?,
    }
    write::write_runtime(root, &old, spec, nl)?;
    write::write_settings(root, &old, spec, nl);

    let doc = reparse(&doc)?;
    match read_spec(&doc, path) {
        Some(got) if got == *spec => Ok(doc),
        got => Err(format!(
            "internal error: the edited file does not read back as the request \
             (wanted {spec:?}, got {got:?})"
        )),
    }
}

fn write_graphql(map: &mut Map, old: &RequestSpec, new: &RequestSpec, nl: &str) -> Result<()> {
    if old.method != new.method {
        map.put("method", &Val::str(&new.method), GRAPHQL_ORDER, false, nl);
    }
    if old.url != new.url {
        map.put("url", &Val::str(&new.url), GRAPHQL_ORDER, false, nl);
    }
    write::sync_list(
        map,
        &ListSpec {
            key: "params",
            order: GRAPHQL_ORDER,
            empty: Empty::Remove,
            read: &read::param,
            same: &|a: &Param, b: &Param| a.name == b.name && a.kind == b.kind,
            render: &write::param_val,
            edit: &write::edit_param,
        },
        &new.params,
        nl,
    );
    write::sync_list(map, &write::header_list(), &new.headers, nl);
    if old.body != new.body {
        let Body::GraphQl { query, variables } = &new.body else {
            return Err("a GraphQL request needs a GraphQL body".into());
        };
        write_body(map, query, variables, nl);
    }
    if old.auth != new.auth {
        write::write_auth(map, &old.auth, &new.auth, nl)?;
    }
    Ok(())
}

/// Edits the selected variant's body, or the body mapping key by key.
fn write_body(map: &mut Map, query: &str, variables: &str, nl: &str) {
    let target: Option<&mut Map> = match map.get_mut("body") {
        Some(node) if node.is_seq() => node.block_seq_mut().and_then(|seq| {
            let selected = seq
                .items
                .iter()
                .position(|i| read::flag(i.value.get("selected")) == Some(true))
                .unwrap_or(0);
            let variant = seq.items.get_mut(selected)?.value.block_map_mut()?;
            if variant.get("body").is_some_and(Node::is_map) {
                variant.get_mut("body").and_then(Node::block_map_mut)
            } else {
                variant.put(
                    "body",
                    &body_val(query, variables),
                    &["title", "selected", "body"],
                    false,
                    nl,
                );
                None
            }
        }),
        Some(node) if node.is_map() => node.block_map_mut(),
        _ => None,
    };
    let Some(body) = target else {
        if !map.get("body").is_some_and(Node::is_seq) {
            map.put(
                "body",
                &body_val(query, variables),
                GRAPHQL_ORDER,
                false,
                nl,
            );
        }
        return;
    };
    if read::text(body.get("query")) != query {
        body.put("query", &Val::str(query), BODY_ORDER, false, nl);
    }
    if read::text(body.get("variables")) != variables {
        body.put("variables", &Val::str(variables), BODY_ORDER, false, nl);
    }
}

/// Turns an `http` item into a `graphql` item or back when the body changes between GraphQL
/// and anything else: `info.type` and the protocol key change, the old body is dropped (the
/// writer adds the new one), everything else stays. `None` when no conversion is needed.
pub(crate) fn convert(doc: &Document, to: &str) -> Option<Result<Document>> {
    let from = doc
        .root
        .get("info")
        .and_then(|i| i.get("type"))
        .and_then(Node::text)?;
    let (from, to) = match (from, to) {
        ("http", "graphql") => ("http", "graphql"),
        ("graphql", "http") => ("graphql", "http"),
        _ => return None,
    };
    let mut doc = doc.clone();
    let nl = doc.newline;
    let Some(root) = doc.root.block_map_mut() else {
        return Some(Err("the file is not a YAML mapping".into()));
    };
    let Some(info) = root.get_mut("info").and_then(Node::block_map_mut) else {
        return Some(Err("`info` is not a block mapping".into()));
    };
    info.put(
        "type",
        &Val::str(to),
        &["name", "type", "seq", "tags"],
        false,
        nl,
    );
    if let Some(entry) = root.entries.iter_mut().find(|e| e.key.value == from) {
        entry.key.value = to.to_owned();
        entry.key.raw = to.to_owned();
        if let Some(map) = entry.value.block_map_mut() {
            map.remove("body");
        }
    }
    Some(reparse(&doc))
}
