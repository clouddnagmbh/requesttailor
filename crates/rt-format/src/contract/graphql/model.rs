// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! One schema out of the definitions of one or more files: extensions merged into their
//! types, the operation root types found.

use std::collections::BTreeMap;

use super::syntax::{Def, DirectiveDef, Document, OpKind, TypeDef, TypeKind};
use crate::contract::{ContractError, SchemaSources};

#[derive(Debug, Default)]
pub(super) struct Schema {
    pub types: BTreeMap<String, TypeDef>,
    pub directives: BTreeMap<String, DirectiveDef>,
    roots: BTreeMap<u8, String>,
}

fn slot(kind: OpKind) -> u8 {
    match kind {
        OpKind::Query => 0,
        OpKind::Mutation => 1,
        OpKind::Subscription => 2,
    }
}

impl Schema {
    /// The root type of an operation kind: named by `schema { … }`, else `Query`, `Mutation`
    /// or `Subscription` if there is such a type.
    pub(super) fn root(&self, kind: OpKind) -> Option<&str> {
        if let Some(name) = self.roots.get(&slot(kind)) {
            return Some(name);
        }
        let default = match kind {
            OpKind::Query => "Query",
            OpKind::Mutation => "Mutation",
            OpKind::Subscription => "Subscription",
        };
        self.types.contains_key(default).then_some(default)
    }

    /// The type a field of `parent` has, by name (without list and non-null wrappers).
    pub(super) fn field_type(&self, parent: &str, field: &str) -> Option<&str> {
        self.types
            .get(parent)?
            .fields
            .iter()
            .find(|f| f.name == field)
            .map(|f| f.ty.named())
    }

    pub(super) fn is_input(&self, name: &str) -> bool {
        self.types
            .get(name)
            .is_some_and(|t| t.kind == TypeKind::Input)
    }

    fn apply(&mut self, defs: Vec<Def>) {
        let mut extensions = Vec::new();
        for def in defs {
            match def {
                Def::Type { def, extend: false } => match self.types.get_mut(&def.name) {
                    Some(existing) => merge(existing, def),
                    None => {
                        self.types.insert(def.name.clone(), def);
                    }
                },
                Def::Type { def, extend: true } => extensions.push(def),
                Def::Directive(d) => {
                    self.directives.insert(d.name.clone(), d);
                }
                Def::Schema(roots) => {
                    for (kind, name) in roots {
                        self.roots.insert(slot(kind), name);
                    }
                }
            }
        }
        for def in extensions {
            match self.types.get_mut(&def.name) {
                Some(existing) => merge(existing, def),
                None => {
                    self.types.insert(def.name.clone(), def);
                }
            }
        }
    }
}

/// `extra` adds to `base` (an extension, or a second definition in another file).
fn merge(base: &mut TypeDef, extra: TypeDef) {
    if base.description.is_none() {
        base.description = extra.description;
    }
    for f in extra.fields {
        match base.fields.iter_mut().find(|x| x.name == f.name) {
            Some(x) => *x = f,
            None => base.fields.push(f),
        }
    }
    for f in extra.inputs {
        match base.inputs.iter_mut().find(|x| x.name == f.name) {
            Some(x) => *x = f,
            None => base.inputs.push(f),
        }
    }
    for i in extra.implements {
        if !base.implements.contains(&i) {
            base.implements.push(i);
        }
    }
    for m in extra.members {
        if !base.members.contains(&m) {
            base.members.push(m);
        }
    }
    for v in extra.values {
        match base.values.iter_mut().find(|x| x.name == v.name) {
            Some(x) => *x = v,
            None => base.values.push(v),
        }
    }
}

/// The schema of `sources` (all files together); `None` is the empty schema. Errors name the
/// file and the line.
pub(super) fn load(sources: Option<&SchemaSources>) -> Result<Schema, ContractError> {
    let mut schema = Schema::default();
    let Some(sources) = sources else {
        return Ok(schema);
    };
    let mut defs = Vec::new();
    for (name, bytes) in sources {
        let doc: Document =
            super::syntax::parse(bytes).map_err(|e| ContractError::Spec(format!("{name}: {e}")))?;
        defs.extend(doc.defs);
    }
    schema.apply(defs);
    Ok(schema)
}
