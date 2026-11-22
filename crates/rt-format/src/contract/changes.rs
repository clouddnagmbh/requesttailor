// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! What changed between two versions of a spec, per operation, with the requests each change
//! affects. Breaking means an existing client can stop working:
//!
//! - an operation or a successful (2xx) response status is removed;
//! - a parameter is removed, or a parameter or request body field becomes required;
//! - a field is removed from a request or response schema;
//! - a type or format changes, except where the new type accepts more (request) or returns
//!   less (response).
//!
//! Everything else is not breaking: additions, optional parameters and fields, new enum
//! values, rewording, `deprecated`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::matching;
use super::schema::{self, Effective};
use super::spec::{Operation, Param, Spec};
use super::{ContractError, FileSet, SpecChange, SpecChangeKind, SpecChanges};
use crate::read_collection;

const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Request,
    Response,
}

/// Request paths per operation key.
fn requests_by_operation(
    spec: &Spec,
    files: &FileSet,
) -> Result<BTreeMap<String, Vec<String>>, ContractError> {
    let doc = read_collection(files)
        .map_err(|e| ContractError::Unsupported(format!("cannot read the collection: {e}")))?;
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in matching::requests(doc.model()) {
        let url = matching::parse_url(&entry.spec.url);
        if let matching::Match::Op(op) = matching::find(spec, &entry.spec.method, &url) {
            map.entry(op.key.clone())
                .or_default()
                .push(entry.path.clone());
        }
    }
    Ok(map)
}

struct Collector<'a> {
    out: Vec<SpecChange>,
    old_reqs: &'a BTreeMap<String, Vec<String>>,
    new_reqs: &'a BTreeMap<String, Vec<String>>,
    removed: bool,
}

impl Collector<'_> {
    fn push(
        &mut self,
        op: &str,
        kind: SpecChangeKind,
        breaking: bool,
        location: String,
        message: String,
    ) {
        let reqs = if self.removed {
            self.old_reqs
        } else {
            self.new_reqs
        };
        self.out.push(SpecChange {
            kind,
            breaking,
            operation: Some(op.to_owned()),
            location,
            message,
            requests: reqs.get(op).cloned().unwrap_or_default(),
        });
    }
}

pub(super) fn changes(
    old: Option<&Spec>,
    new: Option<&Spec>,
    files: &FileSet,
) -> Result<SpecChanges, ContractError> {
    let old_reqs = match old {
        Some(s) => requests_by_operation(s, files)?,
        None => BTreeMap::new(),
    };
    let new_reqs = match new {
        Some(s) => requests_by_operation(s, files)?,
        None => BTreeMap::new(),
    };
    let mut c = Collector {
        out: Vec::new(),
        old_reqs: &old_reqs,
        new_reqs: &new_reqs,
        removed: false,
    };
    let no_ops: &[Operation] = &[];
    let old_ops = old.map_or(no_ops, |s| &s.ops);
    let new_ops = new.map_or(no_ops, |s| &s.ops);
    for n in new_ops {
        match old_ops.iter().find(|o| o.key == n.key) {
            None => {
                c.push(
                    &n.key,
                    SpecChangeKind::OperationAdded,
                    false,
                    String::new(),
                    "operation added".to_owned(),
                );
            }
            Some(o) => {
                // Both specs are present here.
                if let (Some(os), Some(ns)) = (old, new) {
                    diff_operation(&mut c, os, ns, o, n);
                }
            }
        }
    }
    c.removed = true;
    for o in old_ops {
        if !new_ops.iter().any(|n| n.key == o.key) {
            c.push(
                &o.key,
                SpecChangeKind::OperationRemoved,
                true,
                String::new(),
                "operation removed".to_owned(),
            );
        }
    }
    Ok(SpecChanges {
        from_version: old.map(|s| s.version.clone()),
        to_version: new.map(|s| s.version.clone()),
        changes: c.out,
    })
}

fn diff_operation(c: &mut Collector<'_>, os: &Spec, ns: &Spec, o: &Operation, n: &Operation) {
    let key = n.key.as_str();
    if o.summary != n.summary {
        c.push(
            key,
            SpecChangeKind::Other,
            false,
            "summary".into(),
            format!(
                "summary changed from \"{}\" to \"{}\"",
                o.summary, n.summary
            ),
        );
    }
    if o.deprecated != n.deprecated {
        c.push(
            key,
            SpecChangeKind::Other,
            false,
            "deprecated".into(),
            if n.deprecated {
                "marked as deprecated".into()
            } else {
                "no longer deprecated".into()
            },
        );
    }
    diff_params(c, os, ns, o, n);
    diff_body(c, os, ns, o, n);
    diff_responses(c, os, ns, o, n);
}

fn param_loc(p: &Param) -> String {
    format!("{} parameter {}", p.location, p.name)
}

fn diff_params(c: &mut Collector<'_>, os: &Spec, ns: &Spec, o: &Operation, n: &Operation) {
    let key = n.key.as_str();
    for np in &n.params {
        let found = o
            .params
            .iter()
            .find(|p| p.name == np.name && p.location == np.location);
        match found {
            None if np.required => c.push(
                key,
                SpecChangeKind::RequiredAdded,
                true,
                param_loc(np),
                format!("new required {}", param_loc(np)),
            ),
            None => c.push(
                key,
                SpecChangeKind::ParameterAdded,
                false,
                param_loc(np),
                format!("new optional {}", param_loc(np)),
            ),
            Some(op_) => {
                if np.required && !op_.required {
                    c.push(
                        key,
                        SpecChangeKind::RequiredAdded,
                        true,
                        param_loc(np),
                        format!("{} is now required", param_loc(np)),
                    );
                } else if !np.required && op_.required {
                    c.push(
                        key,
                        SpecChangeKind::Other,
                        false,
                        param_loc(np),
                        format!("{} is now optional", param_loc(np)),
                    );
                }
                // The parameter's name stands for its value in the messages.
                let mut sub = Vec::new();
                diff_schema(
                    os,
                    ns,
                    &op_.schema,
                    &np.schema,
                    Side::Request,
                    &np.name,
                    0,
                    &mut sub,
                );
                for (kind, breaking, _field, message) in sub {
                    c.push(key, kind, breaking, param_loc(np), message);
                }
            }
        }
    }
    for op_ in &o.params {
        if !n
            .params
            .iter()
            .any(|p| p.name == op_.name && p.location == op_.location)
        {
            c.push(
                key,
                SpecChangeKind::ParameterRemoved,
                true,
                param_loc(op_),
                format!("{} removed", param_loc(op_)),
            );
        }
    }
}

fn diff_body(c: &mut Collector<'_>, os: &Spec, ns: &Spec, o: &Operation, n: &Operation) {
    let key = n.key.as_str();
    match (&o.body, &n.body) {
        (None, Some(nb)) if nb.required => c.push(
            key,
            SpecChangeKind::RequiredAdded,
            true,
            "request body".into(),
            "a request body is now required".into(),
        ),
        (None, Some(_)) => c.push(
            key,
            SpecChangeKind::Other,
            false,
            "request body".into(),
            "an optional request body was added".into(),
        ),
        (Some(_), None) => c.push(
            key,
            SpecChangeKind::Other,
            false,
            "request body".into(),
            "the request body was removed".into(),
        ),
        (Some(ob), Some(nb)) => {
            if nb.required && !ob.required {
                c.push(
                    key,
                    SpecChangeKind::RequiredAdded,
                    true,
                    "request body".into(),
                    "the request body is now required".into(),
                );
            }
            for (media, nschema) in &nb.content {
                if let Some((_, oschema)) = ob.content.iter().find(|(m, _)| m == media) {
                    diff_schema_into(
                        c,
                        key,
                        os,
                        ns,
                        oschema,
                        nschema,
                        Side::Request,
                        "request body",
                    );
                }
            }
        }
        (None, None) => {}
    }
}

fn diff_responses(c: &mut Collector<'_>, os: &Spec, ns: &Spec, o: &Operation, n: &Operation) {
    let key = n.key.as_str();
    for (status, nmedia) in &n.responses {
        match o.responses.iter().find(|(s, _)| s == status) {
            None => c.push(
                key,
                SpecChangeKind::ResponseChanged,
                false,
                format!("response {status}"),
                format!("response {status} added"),
            ),
            Some((_, omedia)) => {
                for (media, nschema) in nmedia {
                    if let Some((_, oschema)) = omedia.iter().find(|(m, _)| m == media) {
                        diff_schema_into(
                            c,
                            key,
                            os,
                            ns,
                            oschema,
                            nschema,
                            Side::Response,
                            &format!("response {status}"),
                        );
                    }
                }
            }
        }
    }
    for (status, _) in &o.responses {
        if !n.responses.iter().any(|(s, _)| s == status) {
            c.push(
                key,
                SpecChangeKind::ResponseChanged,
                status.starts_with('2'),
                format!("response {status}"),
                format!("response {status} removed"),
            );
        }
    }
}

type Found = (SpecChangeKind, bool, String, String);

#[allow(clippy::too_many_arguments)]
fn diff_schema_into(
    c: &mut Collector<'_>,
    key: &str,
    os: &Spec,
    ns: &Spec,
    old: &Value,
    new: &Value,
    side: Side,
    base: &str,
) {
    let mut found = Vec::new();
    diff_schema(os, ns, old, new, side, "", 0, &mut found);
    for (kind, breaking, field, message) in found {
        let location = if field.is_empty() {
            base.to_owned()
        } else {
            format!("{base}: {field}")
        };
        c.push(key, kind, breaking, location, message);
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

fn names(types: &BTreeSet<String>) -> String {
    types.iter().cloned().collect::<Vec<_>>().join(" or ")
}

#[allow(clippy::too_many_arguments)]
fn diff_schema(
    os: &Spec,
    ns: &Spec,
    old: &Value,
    new: &Value,
    side: Side,
    path: &str,
    depth: usize,
    out: &mut Vec<Found>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let o = schema::effective(&os.root, old);
    let n = schema::effective(&ns.root, new);
    let at = if path.is_empty() {
        "value".to_owned()
    } else {
        format!("`{path}`")
    };

    if !o.types.is_empty() && !n.types.is_empty() && o.types != n.types {
        // Accepting more in a request, returning less in a response, is safe.
        let safe = match side {
            Side::Request => o.types.is_subset(&n.types),
            Side::Response => n.types.is_subset(&o.types),
        };
        out.push((
            SpecChangeKind::TypeChanged,
            !safe,
            path.to_owned(),
            format!(
                "type of {at} changed from {} to {}",
                names(&o.types),
                names(&n.types)
            ),
        ));
    } else if let (Some(of), Some(nf)) = (&o.format, &n.format)
        && of != nf
    {
        out.push((
            SpecChangeKind::TypeChanged,
            true,
            path.to_owned(),
            format!("format of {at} changed from {of} to {nf}"),
        ));
    }
    enums(&o, &n, side, path, &at, out);

    for (name, oschema) in &o.props {
        let sub = join(path, name);
        match n.props.iter().find(|(m, _)| m == name) {
            None => out.push((
                SpecChangeKind::FieldRemoved,
                true,
                sub.clone(),
                format!("field `{sub}` removed"),
            )),
            Some((_, nschema)) => {
                if side == Side::Request && n.required.contains(name) && !o.required.contains(name)
                {
                    out.push((
                        SpecChangeKind::RequiredAdded,
                        true,
                        sub.clone(),
                        format!("field `{sub}` is now required"),
                    ));
                }
                diff_schema(os, ns, oschema, nschema, side, &sub, depth + 1, out);
            }
        }
    }
    for (name, _) in &n.props {
        if o.props.iter().any(|(m, _)| m == name) {
            continue;
        }
        let sub = join(path, name);
        if side == Side::Request && n.required.contains(name) {
            out.push((
                SpecChangeKind::RequiredAdded,
                true,
                sub.clone(),
                format!("new required field `{sub}`"),
            ));
        } else {
            out.push((
                SpecChangeKind::FieldAdded,
                false,
                sub.clone(),
                format!("new field `{sub}`"),
            ));
        }
    }
    if let (Some(oi), Some(ni)) = (&o.items, &n.items) {
        diff_schema(os, ns, oi, ni, side, &format!("{path}[]"), depth + 1, out);
    }
}

fn enums(o: &Effective, n: &Effective, side: Side, path: &str, at: &str, out: &mut Vec<Found>) {
    let (Some(oe), Some(ne)) = (&o.enums, &n.enums) else {
        return;
    };
    let show = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
    for v in oe.iter().filter(|v| !ne.contains(v)) {
        out.push((
            if side == Side::Request {
                SpecChangeKind::TypeChanged
            } else {
                SpecChangeKind::Other
            },
            side == Side::Request,
            path.to_owned(),
            format!("value {} no longer allowed for {at}", show(v)),
        ));
    }
    for v in ne.iter().filter(|v| !oe.contains(v)) {
        out.push((
            SpecChangeKind::Other,
            false,
            path.to_owned(),
            format!("new value {} allowed for {at}", show(v)),
        ));
    }
}
