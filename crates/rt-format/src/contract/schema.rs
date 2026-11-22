// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! JSON Schema as OpenAPI 3.0 and 3.1 use it, as far as the contract check needs: `$ref`
//! (local), `allOf`, types (with `nullable` and `["x", "null"]`), `enum`, `properties`,
//! `required`, `items`, `additionalProperties: false`, `oneOf`/`anyOf` on the validating side.

use std::collections::BTreeSet;

use serde_json::Value;

const MAX_REFS: usize = 32;
const MAX_DEPTH: usize = 16;

/// Follows `$ref`s (`#/…`) from `v`; a reference that cannot be resolved stays as it is.
pub(super) fn resolve<'a>(root: &'a Value, v: &'a Value) -> &'a Value {
    let mut cur = v;
    for _ in 0..MAX_REFS {
        let Some(Value::String(r)) = cur.get("$ref") else {
            return cur;
        };
        let Some(ptr) = r.strip_prefix('#') else {
            return cur;
        };
        match root.pointer(ptr) {
            Some(next) => cur = next,
            None => return cur,
        }
    }
    cur
}

/// A schema with `allOf` merged in.
#[derive(Debug, Default, Clone)]
pub(super) struct Effective {
    pub types: BTreeSet<String>,
    pub format: Option<String>,
    pub props: Vec<(String, Value)>,
    pub required: BTreeSet<String>,
    pub items: Option<Value>,
    pub enums: Option<Vec<Value>>,
    pub no_additional: bool,
    pub alternatives: Vec<Value>,
}

pub(super) fn effective(root: &Value, schema: &Value) -> Effective {
    let mut out = Effective::default();
    merge(root, schema, &mut out, 0);
    out
}

fn merge(root: &Value, schema: &Value, out: &mut Effective, depth: usize) {
    let schema = resolve(root, schema);
    if depth > MAX_DEPTH || !schema.is_object() {
        return;
    }
    match schema.get("type") {
        Some(Value::String(t)) => {
            out.types.insert(t.clone());
        }
        Some(Value::Array(ts)) => {
            for t in ts.iter().filter_map(Value::as_str) {
                out.types.insert(t.to_owned());
            }
        }
        _ => {}
    }
    if schema.get("nullable") == Some(&Value::Bool(true)) {
        out.types.insert("null".into());
    }
    if let Some(Value::String(f)) = schema.get("format") {
        out.format = Some(f.clone());
    }
    if let Some(Value::Object(props)) = schema.get("properties") {
        for (k, v) in props {
            if let Some(slot) = out.props.iter_mut().find(|(n, _)| n == k) {
                slot.1 = v.clone();
            } else {
                out.props.push((k.clone(), v.clone()));
            }
        }
        if out.types.is_empty() {
            out.types.insert("object".into());
        }
    }
    if let Some(Value::Array(req)) = schema.get("required") {
        out.required
            .extend(req.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    if let Some(items) = schema.get("items") {
        out.items = Some(items.clone());
        if out.types.is_empty() {
            out.types.insert("array".into());
        }
    }
    if let Some(Value::Array(e)) = schema.get("enum") {
        out.enums = Some(e.clone());
    }
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        out.no_additional = true;
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(alts)) = schema.get(key) {
            out.alternatives.extend(alts.iter().cloned());
        }
    }
    if let Some(Value::Array(all)) = schema.get("allOf") {
        for part in all {
            merge(root, part, out, depth + 1);
        }
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0) {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_template(v: &Value) -> bool {
    matches!(v, Value::String(s) if s.contains("{{"))
}

fn type_fits(types: &BTreeSet<String>, actual: &str) -> bool {
    types.is_empty() || types.contains(actual) || (actual == "integer" && types.contains("number"))
}

/// The problems of `instance` against `schema`, as `path: message`, at most `limit`.
pub(super) fn validate(
    root: &Value,
    schema: &Value,
    instance: &Value,
    limit: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, schema, instance, "$", 0, &mut out, limit);
    out
}

fn walk(
    root: &Value,
    schema: &Value,
    inst: &Value,
    path: &str,
    depth: usize,
    out: &mut Vec<String>,
    limit: usize,
) {
    if out.len() >= limit || depth > MAX_DEPTH || is_template(inst) {
        return;
    }
    let eff = effective(root, schema);
    let actual = kind_of(inst);
    if !type_fits(&eff.types, actual) {
        let want: Vec<&str> = eff.types.iter().map(String::as_str).collect();
        out.push(format!(
            "{path}: expected {}, found {actual}",
            want.join(" or ")
        ));
        return;
    }
    if let Some(enums) = &eff.enums
        && !enums.contains(inst)
    {
        let list: Vec<String> = enums.iter().map(Value::to_string).collect();
        out.push(format!("{path}: {inst} is not one of {}", list.join(", ")));
    }
    if !eff.alternatives.is_empty() {
        let fits = eff
            .alternatives
            .iter()
            .any(|alt| validate(root, alt, inst, 1).is_empty());
        if !fits {
            out.push(format!("{path}: matches none of the allowed shapes"));
        }
    }
    match inst {
        Value::Object(map) => {
            for name in &eff.required {
                if !map.contains_key(name) {
                    out.push(format!("{path}: required property `{name}` is missing"));
                }
            }
            for (k, v) in map {
                let sub = format!("{path}.{k}");
                if let Some((_, s)) = eff.props.iter().find(|(n, _)| n == k) {
                    walk(root, s, v, &sub, depth + 1, out, limit);
                } else if eff.no_additional {
                    out.push(format!("{sub}: property is not in the schema"));
                }
            }
        }
        Value::Array(list) => {
            if let Some(items) = &eff.items {
                for (i, v) in list.iter().enumerate() {
                    walk(
                        root,
                        items,
                        v,
                        &format!("{path}[{i}]"),
                        depth + 1,
                        out,
                        limit,
                    );
                }
            }
        }
        _ => {}
    }
}
