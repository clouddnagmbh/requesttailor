// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The comparison of two [`ProtoSchema`]s and the gRPC requests each change affects.
//!
//! Operation names: `package.Message`, `package.Message.field`, `package.Enum.VALUE`,
//! `package.Service`, `/package.Service/Method` (as in a request file), `package <name>`.

use std::collections::{BTreeMap, BTreeSet};

use rt_core::model::{Item, Protocol};
use rt_grpc::{Label, ProtoField, ProtoMessage, ProtoSchema};

use crate::FileSet;
use crate::contract::{ContractError, SpecChange, SpecChangeKind};

/// What a change is about, to find the requests it affects.
#[derive(Debug, Clone)]
pub(super) enum Target {
    /// A message or enum: the calls whose request or response reaches it.
    Type(String),
    /// One `rpc` by its path (`/pkg.Service/Method`), old and new spelling.
    Rpc(Vec<String>),
    /// Every `rpc` of a service, by its names.
    Service(Vec<String>),
    None,
}

pub(super) struct Change {
    pub change: SpecChange,
    pub target: Target,
}

#[derive(Default)]
struct Out(Vec<Change>);

impl Out {
    fn push(
        &mut self,
        kind: SpecChangeKind,
        breaking: bool,
        operation: String,
        message: String,
        target: Target,
    ) {
        self.0.push(Change {
            change: SpecChange {
                kind,
                breaking,
                operation: Some(operation),
                location: String::new(),
                message,
                requests: Vec::new(),
            },
            target,
        });
    }
}

/// Names of the old schema as the new one spells them: a file whose package changed keeps its
/// types, only the prefix differs, so they compare as the same types.
#[derive(Default)]
struct Canon {
    rename: BTreeMap<String, String>,
}

impl Canon {
    fn name(&self, name: &str) -> String {
        self.rename
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    }

    /// A field type: a scalar, a name, or `map<k, v>`.
    fn ty(&self, ty: &str) -> String {
        match ty.strip_prefix("map<").and_then(|r| r.strip_suffix('>')) {
            Some(inner) => {
                let (k, v) = inner.split_once(", ").unwrap_or((inner, ""));
                format!("map<{}, {}>", self.name(k), self.name(v))
            }
            None => self.name(ty),
        }
    }
}

fn swap_package(name: &str, old: &str, new: &str) -> String {
    let rest = if old.is_empty() {
        name
    } else {
        name.strip_prefix(old)
            .and_then(|r| r.strip_prefix('.'))
            .unwrap_or(name)
    };
    if new.is_empty() {
        rest.to_owned()
    } else {
        format!("{new}.{rest}")
    }
}

/// The scalar families that read each other's bytes: `int32`, `int64`, `uint32`, `uint64`,
/// `bool` and enums are all varints, and so on (the documented "compatible" pairs).
fn family(ty: &str, schema: &ProtoSchema) -> String {
    match ty {
        "int32" | "int64" | "uint32" | "uint64" | "bool" => "varint".into(),
        "sint32" | "sint64" => "zigzag".into(),
        "fixed32" | "sfixed32" => "fixed32".into(),
        "fixed64" | "sfixed64" => "fixed64".into(),
        "string" | "bytes" => "bytes".into(),
        other if schema.enums.contains_key(other) => "varint".into(),
        other => other.to_owned(),
    }
}

fn label_word(l: Label) -> &'static str {
    match l {
        Label::Singular => "a singular field",
        Label::Optional => "an `optional` field",
        Label::Required => "a `required` field",
        Label::Repeated => "a `repeated` field",
    }
}

fn reserved(m: &ProtoMessage, number: u32, name: &str) -> bool {
    m.reserved_numbers
        .iter()
        .any(|(a, b)| *a <= number && number <= *b)
        || m.reserved_names.iter().any(|n| n == name)
}

fn diff_field(
    out: &mut Out,
    canon: &Canon,
    (old_schema, new_schema): (&ProtoSchema, &ProtoSchema),
    message: &str,
    old: &ProtoField,
    new: &ProtoField,
) {
    let op = format!("{message}.{}", new.name);
    let target = Target::Type(message.to_owned());
    let old_ty = canon.ty(&old.ty);
    if old_ty != new.ty {
        let same_family = family(&old_ty, old_schema) == family(&new.ty, new_schema)
            && !old_ty.starts_with("map<")
            && !schema_message(old_schema, new_schema, &old_ty);
        if same_family && old.label == new.label {
            out.push(
                SpecChangeKind::TypeChanged,
                false,
                op.clone(),
                format!(
                    "type changed from {} to {} (wire-compatible; values may be cut or read differently)",
                    old.ty, new.ty
                ),
                target.clone(),
            );
        } else if same_family {
            out.push(
                SpecChangeKind::TypeChanged,
                false,
                op.clone(),
                format!(
                    "type changed from {} to {} (wire-compatible)",
                    old.ty, new.ty
                ),
                target.clone(),
            );
        } else {
            out.push(
                SpecChangeKind::TypeChanged,
                true,
                op.clone(),
                format!("type changed from {} to {}", old.ty, new.ty),
                target.clone(),
            );
        }
    }
    if old.label != new.label {
        out.push(
            SpecChangeKind::NullabilityChanged,
            true,
            op.clone(),
            format!(
                "changed from {} to {}",
                label_word(old.label),
                label_word(new.label)
            ),
            target.clone(),
        );
    }
    if old.oneof != new.oneof {
        let what = match (&old.oneof, &new.oneof) {
            (Some(a), None) => format!("moved out of oneof {a}"),
            (None, Some(b)) => format!("moved into oneof {b}"),
            (Some(a), Some(b)) => format!("moved from oneof {a} to oneof {b}"),
            (None, None) => String::new(),
        };
        out.push(SpecChangeKind::TypeChanged, true, op, what, target);
    }
}

/// `ty` names a message (they are never wire-compatible with another type but themselves).
fn schema_message(a: &ProtoSchema, b: &ProtoSchema, ty: &str) -> bool {
    a.messages.contains_key(ty) || b.messages.contains_key(ty)
}

fn diff_message(
    out: &mut Out,
    canon: &Canon,
    schemas: (&ProtoSchema, &ProtoSchema),
    name: &str,
    old: &ProtoMessage,
    new: &ProtoMessage,
) {
    let target = Target::Type(name.to_owned());
    let mut handled: BTreeSet<u32> = BTreeSet::new();
    let new_by_name: BTreeMap<&str, &ProtoField> =
        new.fields.values().map(|f| (f.name.as_str(), f)).collect();
    for (number, of) in &old.fields {
        let op = format!("{name}.{}", of.name);
        match new.fields.get(number) {
            Some(nf) if nf.name == of.name => {
                handled.insert(*number);
                diff_field(out, canon, schemas, name, of, nf);
            }
            _ if new_by_name.contains_key(of.name.as_str()) => {
                let moved = new_by_name[of.name.as_str()];
                handled.insert(moved.number);
                out.push(
                    SpecChangeKind::NumberChanged,
                    true,
                    op,
                    format!(
                        "number changed from {number} to {}: data written with the old number is misread",
                        moved.number
                    ),
                    target.clone(),
                );
            }
            Some(nf) => {
                handled.insert(*number);
                let compatible = canon.ty(&of.ty) == nf.ty
                    || (family(&canon.ty(&of.ty), schemas.0) == family(&nf.ty, schemas.1)
                        && !schema_message(schemas.0, schemas.1, &nf.ty));
                if compatible && of.label == nf.label {
                    out.push(
                        SpecChangeKind::Other,
                        false,
                        format!("{name}.{}", nf.name),
                        format!(
                            "field {} renamed to {} (same number and type; the JSON name changes)",
                            of.name, nf.name
                        ),
                        target.clone(),
                    );
                } else {
                    out.push(
                        SpecChangeKind::NumberChanged,
                        true,
                        format!("{name}.{}", nf.name),
                        format!(
                            "number {number} was {} ({}) and is now {} ({}): the number is reused",
                            of.name, of.ty, nf.name, nf.ty
                        ),
                        target.clone(),
                    );
                }
            }
            None => {
                let note = if reserved(new, *number, &of.name) {
                    format!("its number {number} is reserved")
                } else {
                    format!("its number {number} is not reserved (a later field could reuse it)")
                };
                out.push(
                    SpecChangeKind::FieldRemoved,
                    true,
                    op,
                    format!("field {} removed; {note}", of.name),
                    target.clone(),
                );
            }
        }
    }
    for (number, nf) in &new.fields {
        if !handled.contains(number) {
            out.push(
                SpecChangeKind::FieldAdded,
                false,
                format!("{name}.{}", nf.name),
                format!("field {}: {} added", nf.name, nf.ty),
                target.clone(),
            );
        }
    }
}

fn parent_removed(
    name: &str,
    old_messages: &BTreeSet<String>,
    new: &ProtoSchema,
    canon: &Canon,
) -> bool {
    let mut rest = name;
    while let Some((parent, _)) = rest.rsplit_once('.') {
        if old_messages.contains(parent) && !new.messages.contains_key(&canon.name(parent)) {
            return true;
        }
        rest = parent;
    }
    false
}

pub(super) fn diff(old: &ProtoSchema, new: &ProtoSchema) -> Vec<Change> {
    let mut out = Out::default();
    let mut canon = Canon::default();
    // Files whose package changed.
    for (file, o) in &old.files {
        let Some(n) = new.files.get(file) else {
            continue;
        };
        if o.package == n.package {
            continue;
        }
        for (name, m) in &old.messages {
            if m.file == *file {
                canon
                    .rename
                    .insert(name.clone(), swap_package(name, &o.package, &n.package));
            }
        }
        for (name, e) in &old.enums {
            if e.file == *file {
                canon
                    .rename
                    .insert(name.clone(), swap_package(name, &o.package, &n.package));
            }
        }
        let mut names = Vec::new();
        for (name, s) in &old.services {
            if s.file == *file {
                let renamed = swap_package(name, &o.package, &n.package);
                canon.rename.insert(name.clone(), renamed.clone());
                names.push(name.clone());
                names.push(renamed);
            }
        }
        for (name, s) in &new.services {
            if s.file == *file && !names.contains(name) {
                names.push(name.clone());
            }
        }
        out.push(
            SpecChangeKind::TypeChanged,
            true,
            format!("package {}", o.package),
            format!(
                "package changed from {} to {}: the path of every call of its services changes (for example /{}.Svc/M becomes /{}.Svc/M)",
                o.package, n.package, o.package, n.package
            ),
            Target::Service(names),
        );
    }

    let old_message_names: BTreeSet<String> = old.messages.keys().cloned().collect();
    let old_messages: BTreeMap<String, &ProtoMessage> = old
        .messages
        .iter()
        .map(|(k, v)| (canon.name(k), v))
        .collect();
    for (name, n) in &new.messages {
        match old_messages.get(name) {
            None => out.push(
                SpecChangeKind::Other,
                false,
                name.clone(),
                "message added".into(),
                Target::None,
            ),
            Some(o) => diff_message(&mut out, &canon, (old, new), name, o, n),
        }
    }
    for raw in old.messages.keys() {
        let name = canon.name(raw);
        if !new.messages.contains_key(&name)
            && !parent_removed(raw, &old_message_names, new, &canon)
        {
            out.push(
                SpecChangeKind::TypeRemoved,
                true,
                name.clone(),
                format!("message {name} removed"),
                Target::Type(name),
            );
        }
    }

    let old_enums: BTreeMap<String, &rt_grpc::ProtoEnum> =
        old.enums.iter().map(|(k, v)| (canon.name(k), v)).collect();
    for (name, n) in &new.enums {
        match old_enums.get(name) {
            None => out.push(
                SpecChangeKind::Other,
                false,
                name.clone(),
                "enum added".into(),
                Target::None,
            ),
            Some(o) => {
                let target = Target::Type(name.clone());
                for (vname, number) in &o.values {
                    let op = format!("{name}.{vname}");
                    match n.values.iter().find(|(x, _)| x == vname) {
                        Some((_, now)) if now != number => out.push(
                            SpecChangeKind::NumberChanged,
                            true,
                            op,
                            format!("value {vname} changed its number from {number} to {now}"),
                            target.clone(),
                        ),
                        Some(_) => {}
                        None => match n.values.iter().find(|(_, x)| x == number) {
                            Some((renamed, _)) => out.push(
                                SpecChangeKind::Other,
                                false,
                                format!("{name}.{renamed}"),
                                format!("value {vname} renamed to {renamed} (the number is kept; JSON and text use the name)"),
                                target.clone(),
                            ),
                            None => out.push(
                                SpecChangeKind::EnumValueRemoved,
                                true,
                                op,
                                format!("value {vname} = {number} removed"),
                                target.clone(),
                            ),
                        },
                    }
                }
                for (vname, number) in &n.values {
                    let known = o.values.iter().any(|(x, y)| x == vname || y == number);
                    if !known {
                        out.push(
                            SpecChangeKind::Other,
                            false,
                            format!("{name}.{vname}"),
                            format!("value {vname} = {number} added"),
                            target.clone(),
                        );
                    }
                }
            }
        }
    }
    for raw in old.enums.keys() {
        let name = canon.name(raw);
        if !new.enums.contains_key(&name) && !parent_removed(raw, &old_message_names, new, &canon) {
            out.push(
                SpecChangeKind::TypeRemoved,
                true,
                name.clone(),
                format!("enum {name} removed"),
                Target::Type(name),
            );
        }
    }

    let old_services: BTreeMap<String, (&str, &rt_grpc::ProtoService)> = old
        .services
        .iter()
        .map(|(k, v)| (canon.name(k), (k.as_str(), v)))
        .collect();
    for (name, n) in &new.services {
        match old_services.get(name) {
            None => out.push(
                SpecChangeKind::Other,
                false,
                name.clone(),
                "service added".into(),
                Target::None,
            ),
            Some((raw, o)) => {
                for (mname, nm) in &n.methods {
                    let path = format!("/{name}/{mname}");
                    let old_path = format!("/{raw}/{mname}");
                    let target = Target::Rpc(vec![path.clone(), old_path]);
                    match o.methods.get(mname) {
                        None => out.push(
                            SpecChangeKind::OperationAdded,
                            false,
                            path,
                            "rpc added".into(),
                            Target::None,
                        ),
                        Some(om) => {
                            if (om.client_streaming, om.server_streaming)
                                != (nm.client_streaming, nm.server_streaming)
                            {
                                let kind = |c: bool, s: bool| match (c, s) {
                                    (false, false) => "unary",
                                    (true, false) => "client streaming",
                                    (false, true) => "server streaming",
                                    (true, true) => "bidirectional streaming",
                                };
                                out.push(
                                    SpecChangeKind::StreamingChanged,
                                    true,
                                    path.clone(),
                                    format!(
                                        "changed from {} to {}",
                                        kind(om.client_streaming, om.server_streaming),
                                        kind(nm.client_streaming, nm.server_streaming)
                                    ),
                                    target.clone(),
                                );
                            }
                            for (side, o_ty, n_ty) in [
                                ("request", &om.input, &nm.input),
                                ("response", &om.output, &nm.output),
                            ] {
                                if canon.name(o_ty) != *n_ty {
                                    out.push(
                                        SpecChangeKind::TypeChanged,
                                        true,
                                        path.clone(),
                                        format!("{side} message changed from {o_ty} to {n_ty}"),
                                        target.clone(),
                                    );
                                }
                            }
                        }
                    }
                }
                for mname in o.methods.keys() {
                    if !n.methods.contains_key(mname) {
                        let path = format!("/{name}/{mname}");
                        out.push(
                            SpecChangeKind::OperationRemoved,
                            true,
                            path.clone(),
                            "rpc removed".into(),
                            Target::Rpc(vec![path, format!("/{raw}/{mname}")]),
                        );
                    }
                }
            }
        }
    }
    for raw in old.services.keys() {
        let name = canon.name(raw);
        if !new.services.contains_key(&name) {
            out.push(
                SpecChangeKind::TypeRemoved,
                true,
                name.clone(),
                format!("service {name} removed"),
                Target::Service(vec![name, raw.clone()]),
            );
        }
    }
    out.0
}

// ---- requests -------------------------------------------------------------------------

/// One gRPC request of the collection: its path and the method it calls.
pub(super) struct Call {
    path: String,
    method: String,
}

pub(super) fn calls(files: &FileSet) -> Result<Vec<Call>, ContractError> {
    let doc = crate::read_collection(files)
        .map_err(|e| ContractError::Unsupported(format!("cannot read the collection: {e}")))?;
    fn walk(items: &[Item], out: &mut Vec<Call>) {
        for item in items {
            match item {
                Item::Request(r) => {
                    if let Protocol::Grpc(g) = &r.spec.protocol {
                        out.push(Call {
                            path: r.path.clone(),
                            method: g.method.clone(),
                        });
                    }
                }
                Item::Folder(f) => walk(&f.items, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.model().items, &mut out);
    Ok(out)
}

/// Messages and enums a message reaches (itself included).
fn reach(schema: &ProtoSchema, start: &str, into: &mut BTreeSet<String>) {
    if !into.insert(start.to_owned()) {
        return;
    }
    let Some(m) = schema.messages.get(start) else {
        return;
    };
    for f in m.fields.values() {
        match f.ty.strip_prefix("map<").and_then(|r| r.strip_suffix('>')) {
            Some(inner) => {
                for part in inner.split(", ") {
                    reach(schema, part, into);
                }
            }
            None => reach(schema, &f.ty, into),
        }
    }
    // nested types are reached through their parent's fields; a nested enum or message that
    // is declared in this message but used elsewhere is reached from there
}

/// Fills in `SpecChange.requests` of every change.
pub(super) fn assign(changes: &mut [Change], calls: &[Call], old: &ProtoSchema, new: &ProtoSchema) {
    // The types each rpc reaches, in both versions (old names as written; a renamed package
    // matches through the request's own method path).
    let mut rpcs: Vec<(String, BTreeSet<String>)> = Vec::new();
    for schema in [old, new] {
        for (sname, s) in &schema.services {
            for (mname, m) in &s.methods {
                let mut set = BTreeSet::new();
                reach(schema, &m.input, &mut set);
                reach(schema, &m.output, &mut set);
                rpcs.push((format!("/{sname}/{mname}"), set));
            }
        }
    }
    for c in changes {
        let mut found: BTreeSet<&str> = BTreeSet::new();
        match &c.target {
            Target::None => {}
            Target::Rpc(paths) => {
                for call in calls.iter().filter(|x| paths.contains(&x.method)) {
                    found.insert(&call.path);
                }
            }
            Target::Service(names) => {
                for call in calls {
                    if names
                        .iter()
                        .any(|n| call.method.starts_with(&format!("/{n}/")))
                    {
                        found.insert(&call.path);
                    }
                }
            }
            Target::Type(ty) => {
                for (path, set) in &rpcs {
                    if set.contains(ty) {
                        for call in calls.iter().filter(|x| x.method == *path) {
                            found.insert(&call.path);
                        }
                    }
                }
            }
        }
        c.change.requests = found.into_iter().map(str::to_owned).collect();
    }
}
