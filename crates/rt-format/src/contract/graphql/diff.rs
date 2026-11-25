// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What changed between two GraphQL schemas, and which of it breaks clients.
//!
//! Operation names in the result are schema coordinates: `Type`, `Type.field`,
//! `Type.field(arg:)`, `Enum.VALUE`, `@directive`, `@directive(arg:)`.

use std::collections::BTreeMap;

use super::model::Schema;
use super::syntax::{FieldDef, InputValue, Ty, TypeDef, TypeKind};
use crate::contract::{SpecChange, SpecChangeKind};

/// What a change is about, to find the requests it affects.
#[derive(Debug, Clone)]
pub(super) enum Target {
    /// A type as a whole (removed, kind changed, members or interfaces changed).
    Type(String),
    /// A field of an object, interface or input type.
    Field { ty: String, field: String },
    /// An argument; `wide`: every request that selects the field is affected (a new required
    /// argument, a stricter one), else only those that pass the argument.
    Arg {
        ty: String,
        field: String,
        arg: String,
        wide: bool,
    },
    /// Directives and the like: no request is named.
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

/// How two type references differ.
#[derive(Default)]
struct Rel {
    /// A `!` the new one has and the old did not.
    stricter: bool,
    /// A `!` the old one had and the new does not.
    looser: bool,
    /// Another name, or a list against a single value.
    other: bool,
}

fn rel(old: &Ty, new: &Ty, r: &mut Rel) {
    match (old, new) {
        (Ty::NonNull(a), Ty::NonNull(b)) => rel(a, b, r),
        (Ty::NonNull(a), b) => {
            r.looser = true;
            rel(a, b, r);
        }
        (a, Ty::NonNull(b)) => {
            r.stricter = true;
            rel(a, b, r);
        }
        (Ty::List(a), Ty::List(b)) => rel(a, b, r),
        (Ty::Named(a), Ty::Named(b)) => r.other |= a != b,
        _ => r.other = true,
    }
}

/// Where a type is used: what a server sends, or what a client sends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Output,
    Input,
}

fn compare_types(
    out: &mut Out,
    op: &str,
    what: &str,
    old: &Ty,
    new: &Ty,
    dir: Direction,
    target: &Target,
) {
    if old == new {
        return;
    }
    let mut r = Rel::default();
    rel(old, new, &mut r);
    if r.other {
        out.push(
            SpecChangeKind::TypeChanged,
            true,
            op.to_owned(),
            format!("{what} type changed from {old} to {new}"),
            target.clone(),
        );
        return;
    }
    let breaking = match dir {
        Direction::Output => r.looser,
        Direction::Input => r.stricter,
    };
    let note = if breaking {
        match dir {
            Direction::Output => "may now be null",
            Direction::Input => "is now required",
        }
    } else {
        match dir {
            Direction::Output => "is now never null",
            Direction::Input => "is now optional",
        }
    };
    out.push(
        SpecChangeKind::NullabilityChanged,
        breaking,
        op.to_owned(),
        format!("{what} {note}: {old} became {new}"),
        target.clone(),
    );
}

fn describe(out: &mut Out, op: &str, old: &Option<String>, new: &Option<String>, target: &Target) {
    if old != new {
        out.push(
            SpecChangeKind::Other,
            false,
            op.to_owned(),
            "description changed".into(),
            target.clone(),
        );
    }
}

fn by_name<T>(items: &[T], name: impl Fn(&T) -> &str) -> BTreeMap<&str, &T> {
    items.iter().map(|i| (name(i), i)).collect()
}

fn diff_args(out: &mut Out, ty: &str, field: &str, old: &[InputValue], new: &[InputValue]) {
    let (o, n) = (by_name(old, |a| &a.name), by_name(new, |a| &a.name));
    let prefix = if ty.is_empty() {
        format!("@{field}")
    } else {
        format!("{ty}.{field}")
    };
    let coordinate = |arg: &str| format!("{prefix}({arg}:)");
    let target = |arg: &str, wide: bool| {
        if ty.is_empty() {
            Target::None
        } else {
            Target::Arg {
                ty: ty.to_owned(),
                field: field.to_owned(),
                arg: arg.to_owned(),
                wide,
            }
        }
    };
    for (name, a) in &n {
        match o.get(name) {
            None if a.required() => out.push(
                SpecChangeKind::RequiredAdded,
                true,
                coordinate(name),
                format!("new required argument {name}: {}", a.ty),
                target(name, true),
            ),
            None => out.push(
                SpecChangeKind::ParameterAdded,
                false,
                coordinate(name),
                format!("argument {name}: {} added", a.ty),
                target(name, false),
            ),
            Some(before) => {
                let mut r = Rel::default();
                rel(&before.ty, &a.ty, &mut r);
                // a stricter argument breaks every request of the field, a changed type only
                // those that pass it
                let wide = r.stricter && !r.other;
                compare_types(
                    out,
                    &coordinate(name),
                    &format!("argument {name}"),
                    &before.ty,
                    &a.ty,
                    Direction::Input,
                    &target(name, wide),
                );
                if a.deprecated && !before.deprecated {
                    out.push(
                        SpecChangeKind::Other,
                        false,
                        coordinate(name),
                        format!("argument {name} is deprecated"),
                        target(name, false),
                    );
                }
            }
        }
    }
    for name in o.keys() {
        if !n.contains_key(name) {
            out.push(
                SpecChangeKind::ParameterRemoved,
                true,
                coordinate(name),
                format!("argument {name} removed"),
                target(name, false),
            );
        }
    }
}

fn diff_field(out: &mut Out, ty: &str, old: &FieldDef, new: &FieldDef) {
    let op = format!("{ty}.{}", new.name);
    let target = Target::Field {
        ty: ty.to_owned(),
        field: new.name.clone(),
    };
    compare_types(
        out,
        &op,
        "field",
        &old.ty,
        &new.ty,
        Direction::Output,
        &target,
    );
    if new.deprecated && !old.deprecated {
        out.push(
            SpecChangeKind::Other,
            false,
            op.clone(),
            "field is deprecated".into(),
            target.clone(),
        );
    }
    describe(out, &op, &old.description, &new.description, &target);
    diff_args(out, ty, &new.name, &old.args, &new.args);
}

fn diff_input_field(out: &mut Out, ty: &str, old: &InputValue, new: &InputValue) {
    let op = format!("{ty}.{}", new.name);
    let target = Target::Field {
        ty: ty.to_owned(),
        field: new.name.clone(),
    };
    compare_types(
        out,
        &op,
        "field",
        &old.ty,
        &new.ty,
        Direction::Input,
        &target,
    );
    if new.deprecated && !old.deprecated {
        out.push(
            SpecChangeKind::Other,
            false,
            op.clone(),
            "field is deprecated".into(),
            target.clone(),
        );
    }
    describe(out, &op, &old.description, &new.description, &target);
}

fn names_of(list: &[String]) -> BTreeMap<&str, ()> {
    list.iter().map(|s| (s.as_str(), ())).collect()
}

fn diff_type(out: &mut Out, old: &TypeDef, new: &TypeDef) {
    let name = &new.name;
    let whole = Target::Type(name.clone());
    if old.kind != new.kind {
        out.push(
            SpecChangeKind::TypeChanged,
            true,
            name.clone(),
            format!("changed from {} to {}", old.kind.word(), new.kind.word()),
            whole,
        );
        return;
    }
    describe(out, name, &old.description, &new.description, &whole);
    // interfaces an object or interface implements
    let (oi, ni) = (names_of(&old.implements), names_of(&new.implements));
    for i in oi.keys().filter(|i| !ni.contains_key(*i)) {
        out.push(
            SpecChangeKind::TypeChanged,
            true,
            name.clone(),
            format!("no longer implements {i}"),
            whole.clone(),
        );
    }
    for i in ni.keys().filter(|i| !oi.contains_key(*i)) {
        out.push(
            SpecChangeKind::Other,
            false,
            name.clone(),
            format!("now implements {i}"),
            whole.clone(),
        );
    }
    match new.kind {
        TypeKind::Object | TypeKind::Interface => {
            let (of, nf) = (
                by_name(&old.fields, |f| &f.name),
                by_name(&new.fields, |f| &f.name),
            );
            for (fname, f) in &nf {
                match of.get(fname) {
                    None => out.push(
                        SpecChangeKind::FieldAdded,
                        false,
                        format!("{name}.{fname}"),
                        format!("field {fname}: {} added", f.ty),
                        Target::None,
                    ),
                    Some(before) => diff_field(out, name, before, f),
                }
            }
            for fname in of.keys().filter(|f| !nf.contains_key(*f)) {
                out.push(
                    SpecChangeKind::FieldRemoved,
                    true,
                    format!("{name}.{fname}"),
                    format!("field {fname} removed"),
                    Target::Field {
                        ty: name.clone(),
                        field: (*fname).to_owned(),
                    },
                );
            }
        }
        TypeKind::Input => {
            let (of, nf) = (
                by_name(&old.inputs, |f| &f.name),
                by_name(&new.inputs, |f| &f.name),
            );
            for (fname, f) in &nf {
                let coordinate = format!("{name}.{fname}");
                let target = Target::Field {
                    ty: name.clone(),
                    field: (*fname).to_owned(),
                };
                match of.get(fname) {
                    None if f.required() => out.push(
                        SpecChangeKind::RequiredAdded,
                        true,
                        coordinate,
                        format!("new required field {fname}: {}", f.ty),
                        target,
                    ),
                    None => out.push(
                        SpecChangeKind::FieldAdded,
                        false,
                        coordinate,
                        format!("field {fname}: {} added", f.ty),
                        target,
                    ),
                    Some(before) => diff_input_field(out, name, before, f),
                }
            }
            for fname in of.keys().filter(|f| !nf.contains_key(*f)) {
                out.push(
                    SpecChangeKind::FieldRemoved,
                    true,
                    format!("{name}.{fname}"),
                    format!("field {fname} removed"),
                    Target::Field {
                        ty: name.clone(),
                        field: (*fname).to_owned(),
                    },
                );
            }
        }
        TypeKind::Union => {
            let (om, nm) = (names_of(&old.members), names_of(&new.members));
            for m in om.keys().filter(|m| !nm.contains_key(*m)) {
                out.push(
                    SpecChangeKind::TypeChanged,
                    true,
                    name.clone(),
                    format!("member {m} removed from the union"),
                    whole.clone(),
                );
            }
            for m in nm.keys().filter(|m| !om.contains_key(*m)) {
                out.push(
                    SpecChangeKind::Other,
                    false,
                    name.clone(),
                    format!("member {m} added to the union"),
                    whole.clone(),
                );
            }
        }
        TypeKind::Enum => {
            let (ov, nv) = (
                by_name(&old.values, |v| &v.name),
                by_name(&new.values, |v| &v.name),
            );
            for (vname, v) in &nv {
                let coordinate = format!("{name}.{vname}");
                match ov.get(vname) {
                    None => out.push(
                        SpecChangeKind::Other,
                        false,
                        coordinate,
                        format!("value {vname} added (a client that lists every value may need to handle it)"),
                        whole.clone(),
                    ),
                    Some(before) => {
                        if v.deprecated && !before.deprecated {
                            out.push(
                                SpecChangeKind::Other,
                                false,
                                coordinate.clone(),
                                format!("value {vname} is deprecated"),
                                whole.clone(),
                            );
                        }
                        describe(out, &coordinate, &before.description, &v.description, &whole);
                    }
                }
            }
            for vname in ov.keys().filter(|v| !nv.contains_key(*v)) {
                out.push(
                    SpecChangeKind::EnumValueRemoved,
                    true,
                    format!("{name}.{vname}"),
                    format!("value {vname} removed"),
                    whole.clone(),
                );
            }
        }
        TypeKind::Scalar => {}
    }
}

pub(super) fn diff(old: &Schema, new: &Schema) -> Vec<Change> {
    let mut out = Out::default();
    for (name, n) in &new.types {
        match old.types.get(name) {
            None => out.push(
                SpecChangeKind::Other,
                false,
                name.clone(),
                format!("{} added", n.kind.word()),
                Target::None,
            ),
            Some(o) => diff_type(&mut out, o, n),
        }
    }
    for (name, o) in &old.types {
        if !new.types.contains_key(name) {
            out.push(
                SpecChangeKind::TypeRemoved,
                true,
                name.clone(),
                format!("{} {name} removed", o.kind.word()),
                Target::Type(name.clone()),
            );
        }
    }
    for (name, n) in &new.directives {
        let op = format!("@{name}");
        match old.directives.get(name) {
            None => out.push(
                SpecChangeKind::Other,
                false,
                op,
                "directive added".into(),
                Target::None,
            ),
            Some(o) => diff_args(&mut out, "", name, &o.args, &n.args),
        }
    }
    for name in old.directives.keys() {
        if !new.directives.contains_key(name) {
            out.push(
                SpecChangeKind::TypeRemoved,
                true,
                format!("@{name}"),
                format!("directive @{name} removed"),
                Target::None,
            );
        }
    }
    out.0
}
