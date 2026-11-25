// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Which parts of a schema the GraphQL requests of a collection use, found by walking each
//! request's operations through the schema: the fields it selects, the arguments it passes and
//! the types it reaches (also through fragments and variables). A query that does not parse
//! (it may hold `{{templates}}`) is matched by the words in it instead.

use std::collections::{BTreeMap, BTreeSet};

use rt_core::model::{Body, Item};

use super::diff::{Change, Target};
use super::model::Schema;
use super::syntax::{Document, Sel};
use crate::FileSet;
use crate::contract::ContractError;

/// What one request uses.
#[derive(Debug, Default)]
pub(super) struct Usage {
    /// `Type.field` and `Type.field(arg:)`.
    coords: BTreeSet<String>,
    /// Types reached.
    types: BTreeSet<String>,
    /// Set when the query did not parse: every word of it.
    words: Option<BTreeSet<String>>,
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn walk(
    schema: &Schema,
    doc: &Document,
    sels: &[Sel],
    parent: Option<&str>,
    usage: &mut Usage,
    expanded: &mut BTreeSet<String>,
) {
    for sel in sels {
        match sel {
            Sel::Field { name, args, sel } => {
                if name.starts_with("__") {
                    continue;
                }
                let child = parent.and_then(|p| {
                    usage.types.insert(p.to_owned());
                    usage.coords.insert(format!("{p}.{name}"));
                    for a in args {
                        usage.coords.insert(format!("{p}.{name}({a}:)"));
                    }
                    schema.field_type(p, name)
                });
                if let Some(c) = child {
                    usage.types.insert(c.to_owned());
                }
                walk(schema, doc, sel, child, usage, expanded);
            }
            Sel::Inline { on, sel } => {
                if let Some(on) = on {
                    usage.types.insert(on.clone());
                }
                walk(schema, doc, sel, on.as_deref().or(parent), usage, expanded);
            }
            Sel::Spread(name) => {
                let Some(fragment) = doc.fragments.get(name) else {
                    continue;
                };
                usage.types.insert(fragment.on.clone());
                if expanded.insert(format!("{name}@{}", parent.unwrap_or(""))) {
                    walk(
                        schema,
                        doc,
                        &fragment.sel,
                        Some(&fragment.on),
                        usage,
                        expanded,
                    );
                }
            }
        }
    }
}

fn usage_of(schema: &Schema, query: &str) -> Usage {
    let doc = match super::syntax::parse(query.as_bytes()) {
        Ok(d) if !d.operations.is_empty() => d,
        _ => {
            return Usage {
                words: Some(words(query)),
                ..Usage::default()
            };
        }
    };
    let mut usage = Usage::default();
    for op in &doc.operations {
        for v in &op.vars {
            usage.types.insert(v.clone());
        }
        let root = schema.root(op.kind);
        if let Some(r) = root {
            usage.types.insert(r.to_owned());
        }
        let mut expanded = BTreeSet::new();
        walk(schema, &doc, &op.sel, root, &mut usage, &mut expanded);
    }
    usage
}

/// One GraphQL request: its path and what it uses in the old and the new schema.
pub(super) struct RequestUsage {
    path: String,
    old: Usage,
    new: Usage,
}

pub(super) fn requests(
    files: &FileSet,
    old: &Schema,
    new: &Schema,
) -> Result<Vec<RequestUsage>, ContractError> {
    let doc = crate::read_collection(files)
        .map_err(|e| ContractError::Unsupported(format!("cannot read the collection: {e}")))?;
    let mut out = Vec::new();
    fn walk_items(items: &[Item], out: &mut Vec<(String, String)>) {
        for item in items {
            match item {
                Item::Request(r) => {
                    if let Body::GraphQl { query, .. } = &r.spec.body {
                        out.push((r.path.clone(), query.clone()));
                    }
                }
                Item::Folder(f) => walk_items(&f.items, out),
            }
        }
    }
    let mut found = Vec::new();
    walk_items(&doc.model().items, &mut found);
    for (path, query) in found {
        out.push(RequestUsage {
            old: usage_of(old, &query),
            new: usage_of(new, &query),
            path,
        });
    }
    Ok(out)
}

impl Usage {
    fn hits(&self, target: &Target, schemas: &[&Schema]) -> bool {
        match target {
            Target::None => false,
            Target::Type(t) => {
                self.types.contains(t) || self.words.as_ref().is_some_and(|w| w.contains(t))
            }
            Target::Field { ty, field } => {
                let input = schemas.iter().any(|s| s.is_input(ty));
                if input && self.types.contains(ty) {
                    return true;
                }
                self.coords.contains(&format!("{ty}.{field}"))
                    || self.words.as_ref().is_some_and(|w| w.contains(field))
            }
            Target::Arg {
                ty,
                field,
                arg,
                wide,
            } => {
                let coordinate = if *wide {
                    format!("{ty}.{field}")
                } else {
                    format!("{ty}.{field}({arg}:)")
                };
                self.coords.contains(&coordinate)
                    || self
                        .words
                        .as_ref()
                        .is_some_and(|w| w.contains(field) && (*wide || w.contains(arg)))
            }
        }
    }
}

/// Fills in `SpecChange.requests` of every change.
pub(super) fn assign(changes: &mut [Change], used: &[RequestUsage], old: &Schema, new: &Schema) {
    let schemas = [old, new];
    let mut cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in changes {
        let key = format!("{:?}", c.target);
        let list = cache.entry(key).or_insert_with(|| {
            used.iter()
                .filter(|u| u.old.hits(&c.target, &schemas) || u.new.hits(&c.target, &schemas))
                .map(|u| u.path.clone())
                .collect()
        });
        c.change.requests = list.clone();
    }
}
