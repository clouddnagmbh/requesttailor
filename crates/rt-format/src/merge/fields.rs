// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Three-way merge of one model value (request, folder, collection, environment), field by
//! field, on its serde JSON. The field paths are those of [`crate::diff`] (see the table
//! there): scalars by name, list entries by their key (header name, parameter kind and name,
//! variable name, script phase, assertion expression, action variable), auth and protocol per
//! field while the type stays, everything else as a whole.
//!
//! A field merges when at most one side changed it, or both changed it the same way.
//! Otherwise it is a conflict: the merged value keeps ours, unless a [`ConflictChoice`] picks
//! a side for that field.

use serde_json::{Map, Value};

use super::{ConflictChoice, Side};
use crate::diff::{ChangeTarget, DiffValue, FieldPath};

/// A conflict as found, before its values become [`DiffValue`]s.
#[derive(Debug, Clone)]
pub(crate) struct RawConflict {
    pub field: FieldPath,
    pub base: Option<Value>,
    pub ours: Option<Value>,
    pub theirs: Option<Value>,
    /// A secret variable on some side: values are never shown.
    pub secret: bool,
}

impl RawConflict {
    /// The three values for display. A list entry whose sides differ only in `value` shows
    /// that value as text; a secret shows as [`DiffValue::Secret`].
    pub fn values(&self) -> [Option<DiffValue>; 3] {
        let sides = [&self.base, &self.ours, &self.theirs];
        if self.secret {
            return sides.map(|v| present(v).map(|_| DiffValue::Secret));
        }
        let only_value = {
            let rest = |v: &Value| {
                v.as_object()
                    .filter(|o| o.get("value").is_some_and(Value::is_string))
                    .map(|o| {
                        let mut o = o.clone();
                        o.remove("value");
                        o
                    })
            };
            let present: Vec<&Value> = sides.iter().filter_map(|v| present(v)).collect();
            let first = present.first().and_then(|v| rest(v));
            first.is_some() && present.iter().all(|v| rest(v) == first)
        };
        sides.map(|v| {
            present(v).map(|v| match v {
                Value::String(s) => DiffValue::Text { text: s.clone() },
                Value::Object(o) if only_value => DiffValue::Text {
                    text: o["value"].as_str().unwrap_or_default().to_owned(),
                },
                other => DiffValue::Json {
                    value: other.clone(),
                },
            })
        })
    }
}

/// `null` counts as "not there".
fn present(v: &Option<Value>) -> Option<&Value> {
    v.as_ref().filter(|v| !v.is_null())
}

fn norm(v: Option<&Value>) -> Option<&Value> {
    v.filter(|v| !v.is_null())
}

pub(crate) struct Merger<'a> {
    target: &'a ChangeTarget,
    choices: &'a [ConflictChoice],
    pub conflicts: Vec<RawConflict>,
}

/// Merges one key of an object: key, field path, base, ours, theirs.
type EachFn<M> = dyn Fn(
    &mut M,
    &str,
    FieldPath,
    Option<&Value>,
    Option<&Value>,
    Option<&Value>,
) -> Option<Value>;

/// How a list's entries are named in field paths.
type KeyFn = fn(&Value) -> Vec<String>;

fn by(field: &'static str) -> impl Fn(&Value) -> String {
    move |v: &Value| match v.get(field) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn key_name(v: &Value) -> Vec<String> {
    vec![by("name")(v)]
}
fn key_param(v: &Value) -> Vec<String> {
    vec![by("kind")(v), by("name")(v)]
}
fn key_phase(v: &Value) -> Vec<String> {
    vec![by("phase")(v)]
}
fn key_expression(v: &Value) -> Vec<String> {
    vec![by("expression")(v)]
}
fn key_variable(v: &Value) -> Vec<String> {
    vec![by("variable")(v)]
}

fn is_secret(v: Option<&Value>) -> bool {
    v.and_then(|v| v.get("secret"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn child(prefix: &FieldPath, seg: &str) -> FieldPath {
    let mut p = prefix.clone();
    p.push(seg.to_owned());
    p
}

impl<'a> Merger<'a> {
    pub fn new(target: &'a ChangeTarget, choices: &'a [ConflictChoice]) -> Self {
        Self {
            target,
            choices,
            conflicts: Vec::new(),
        }
    }

    fn choice(&self, field: &FieldPath) -> Option<Side> {
        self.choices.iter().find_map(|c| match c {
            ConflictChoice::Field {
                target,
                field: f,
                side,
            } if target == self.target && f == field => Some(*side),
            _ => None,
        })
    }

    /// A value merged as a whole.
    fn atomic(
        &mut self,
        field: FieldPath,
        b: Option<&Value>,
        o: Option<&Value>,
        t: Option<&Value>,
        secret: bool,
    ) -> Option<Value> {
        let (nb, no, nt) = (norm(b), norm(o), norm(t));
        if no == nt || nb == nt {
            return o.cloned();
        }
        if nb == no {
            return t.cloned();
        }
        match self.choice(&field) {
            Some(Side::Ours) => o.cloned(),
            Some(Side::Theirs) => t.cloned(),
            None => {
                self.conflicts.push(RawConflict {
                    field,
                    base: b.cloned(),
                    ours: o.cloned(),
                    theirs: t.cloned(),
                    secret,
                });
                o.cloned()
            }
        }
    }

    /// An object merged key by key; `each` merges one key. Keys keep ours' order, keys only
    /// theirs has follow.
    fn object(
        &mut self,
        prefix: &FieldPath,
        b: Option<&Value>,
        o: &Map<String, Value>,
        t: &Map<String, Value>,
        each: &EachFn<Self>,
    ) -> Value {
        let b = b.and_then(Value::as_object);
        let mut keys: Vec<&String> = o.keys().collect();
        for k in t.keys().chain(b.into_iter().flat_map(|b| b.keys())) {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        let mut out = Map::new();
        for k in keys {
            let merged = each(
                self,
                k,
                child(prefix, k),
                b.and_then(|b| b.get(k)),
                o.get(k),
                t.get(k),
            );
            if let Some(v) = merged {
                out.insert(k.clone(), v);
            }
        }
        Value::Object(out)
    }

    /// Objects of the same type (`tag`) on every side merged per field, else as a whole.
    fn tagged(
        &mut self,
        prefix: FieldPath,
        tag: &str,
        b: Option<&Value>,
        o: Option<&Value>,
        t: Option<&Value>,
    ) -> Option<Value> {
        let kind = |v: Option<&Value>| v.and_then(|v| v.get(tag)).cloned();
        if let (Some(Value::Object(oo)), Some(Value::Object(to))) = (o, t)
            && kind(o) == kind(t)
            && b.is_none_or(|b| b.is_object() && kind(Some(b)) == kind(o))
            && o != t
        {
            return Some(self.object(&prefix, b, oo, to, &|m, _, path, b, o, t| {
                m.atomic(path, b, o, t, false)
            }));
        }
        self.atomic(prefix, b, o, t, false)
    }

    /// A list merged entry by entry, entries named by `key` (a repeated key gets `#2`, `#3`,
    /// … on its last segment). Ours' order is kept; entries only theirs added follow the
    /// entry they follow in theirs.
    fn keyed(
        &mut self,
        prefix: FieldPath,
        b: Option<&Value>,
        o: Option<&Value>,
        t: Option<&Value>,
        key: KeyFn,
        secrets: bool,
    ) -> Option<Value> {
        let (Some(Value::Array(oa)), Some(Value::Array(ta))) = (o, t) else {
            return self.atomic(prefix, b, o, t, false);
        };
        let ba = match b {
            Some(Value::Array(a)) => Some(a),
            None => None,
            Some(_) => return self.atomic(prefix, b, o, t, false),
        };
        if oa == ta {
            return o.cloned();
        }
        fn named(list: &[Value], key: KeyFn) -> Vec<(Vec<String>, &Value)> {
            let mut seen: Vec<Vec<String>> = Vec::new();
            list.iter()
                .map(|v| {
                    let k = key(v);
                    let n = seen.iter().filter(|s| **s == k).count();
                    seen.push(k.clone());
                    let mut k = k;
                    if n > 0
                        && let Some(last) = k.last_mut()
                    {
                        last.push_str(&format!("#{}", n + 1));
                    }
                    (k, v)
                })
                .collect()
        }
        let bn = ba.map(|a| named(a, key)).unwrap_or_default();
        let on = named(oa, key);
        let tn = named(ta, key);
        fn find<'v>(list: &[(Vec<String>, &'v Value)], k: &[String]) -> Option<&'v Value> {
            list.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
        }
        let mut keys: Vec<Vec<String>> = on.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in tn.iter().chain(bn.iter()) {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        let mut merged: Vec<(Vec<String>, Value)> = Vec::new();
        for k in &keys {
            let (bv, ov, tv) = (find(&bn, k), find(&on, k), find(&tn, k));
            let mut path = prefix.clone();
            path.extend(k.iter().cloned());
            let secret = secrets && (is_secret(bv) || is_secret(ov) || is_secret(tv));
            if let Some(v) = self.atomic(path, bv, ov, tv, secret) {
                merged.push((k.clone(), v));
            }
        }
        // Order: ours' entries first, then the others after their predecessor in theirs.
        let mut out: Vec<(Vec<String>, Value)> = Vec::new();
        for (k, _) in &on {
            if let Some(i) = merged.iter().position(|(m, _)| m == k) {
                out.push(merged.remove(i));
            }
        }
        for (k, v) in merged {
            let before = tn.iter().position(|(n, _)| *n == k).and_then(|i| {
                tn[..i]
                    .iter()
                    .rev()
                    .find_map(|(p, _)| out.iter().position(|(o, _)| o == p))
            });
            match before {
                Some(i) => out.insert(i + 1, (k, v)),
                None if tn.iter().any(|(n, _)| *n == k) => out.insert(0, (k, v)),
                None => out.push((k, v)),
            }
        }
        Some(Value::Array(out.into_iter().map(|(_, v)| v).collect()))
    }

    /// One field of request defaults or a request (`headers`, `vars`, `auth`, …).
    fn spec_field(
        &mut self,
        key: &str,
        path: FieldPath,
        b: Option<&Value>,
        o: Option<&Value>,
        t: Option<&Value>,
    ) -> Option<Value> {
        match key {
            "headers" => self.keyed(path, b, o, t, key_name, false),
            "params" => self.keyed(path, b, o, t, key_param, false),
            "vars" => self.keyed(path, b, o, t, key_name, true),
            "scripts" => self.keyed(path, b, o, t, key_phase, false),
            "assertions" => self.keyed(path, b, o, t, key_expression, false),
            "actions" => self.keyed(path, b, o, t, key_variable, false),
            "auth" | "protocol" => self.tagged(path, "type", b, o, t),
            "settings" => match (o, t) {
                (Some(Value::Object(oo)), Some(Value::Object(to))) => {
                    Some(self.object(&path, b, oo, to, &|m, _, p, b, o, t| {
                        m.atomic(p, b, o, t, false)
                    }))
                }
                _ => self.atomic(path, b, o, t, false),
            },
            _ => self.atomic(path, b, o, t, false),
        }
    }

    /// A request (`RequestSpec` JSON).
    pub fn request(&mut self, b: Option<&Value>, o: &Value, t: &Value) -> Option<Value> {
        let (Value::Object(oo), Value::Object(to)) = (o, t) else {
            return None;
        };
        Some(
            self.object(&Vec::new(), b, oo, to, &|m, key, path, b, o, t| {
                m.spec_field(key, path, b, o, t)
            }),
        )
    }

    /// A folder or the collection: `name`, `seq`, `defaults` (request fields under
    /// `["defaults", …]`), `config` (per key), inline `environments` (each as a whole).
    pub fn container(&mut self, b: Option<&Value>, o: &Value, t: &Value) -> Option<Value> {
        let (Value::Object(oo), Value::Object(to)) = (o, t) else {
            return None;
        };
        Some(self.object(
            &Vec::new(),
            b,
            oo,
            to,
            &|m, key, path, b, o, t| match (key, o, t) {
                ("defaults", Some(Value::Object(od)), Some(Value::Object(td))) => {
                    Some(m.object(&path, b, od, td, &|m, key, path, b, o, t| {
                        m.spec_field(key, path, b, o, t)
                    }))
                }
                ("config", Some(Value::Object(oc)), Some(Value::Object(tc))) => {
                    Some(m.object(&path, b, oc, tc, &|m, _, p, b, o, t| {
                        m.atomic(p, b, o, t, false)
                    }))
                }
                ("environments", ..) => m.keyed(path, b, o, t, key_name, false),
                _ => m.atomic(path, b, o, t, false),
            },
        ))
    }

    /// An environment: `name`, `variables` (by name, secrets never shown), the rest as a
    /// whole.
    pub fn environment(&mut self, b: Option<&Value>, o: &Value, t: &Value) -> Option<Value> {
        let (Value::Object(oo), Value::Object(to)) = (o, t) else {
            return None;
        };
        Some(
            self.object(&Vec::new(), b, oo, to, &|m, key, path, b, o, t| match key {
                "variables" => m.keyed(path, b, o, t, key_name, true),
                _ => m.atomic(path, b, o, t, false),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(b: Value, o: Value, t: Value) -> (Value, Vec<RawConflict>) {
        let target = ChangeTarget::Collection;
        let mut m = Merger::new(&target, &[]);
        let v = m.request(Some(&b), &o, &t).unwrap();
        (v, m.conflicts)
    }

    #[test]
    fn lists_merge_by_key_and_keep_order() {
        let h = |n: &str, v: &str| json!({"name": n, "value": v, "enabled": true});
        let b = json!({"headers": [h("A", "1"), h("B", "1")]});
        let o = json!({"headers": [h("A", "2"), h("B", "1"), h("C", "1")]});
        let t = json!({"headers": [h("X", "1"), h("A", "1")]});
        let (v, c) = run(b, o, t);
        assert!(c.is_empty());
        assert_eq!(
            v,
            json!({"headers": [h("X", "1"), h("A", "2"), h("C", "1")]})
        );
    }

    #[test]
    fn the_same_entry_changed_twice_conflicts_by_name() {
        let h = |v: &str| json!({"name": "A", "value": v, "enabled": true});
        let (v, c) = run(
            json!({"headers": [h("1")]}),
            json!({"headers": [h("2")]}),
            json!({"headers": [h("3")]}),
        );
        assert_eq!(v, json!({"headers": [h("2")]}));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].field, vec!["headers", "A"]);
        assert_eq!(c[0].values()[2], Some(DiffValue::Text { text: "3".into() }));
    }

    #[test]
    fn auth_fields_merge_while_the_type_stays() {
        let (v, c) = run(
            json!({"auth": {"type": "basic", "username": "u", "password": "p"}}),
            json!({"auth": {"type": "basic", "username": "U", "password": "p"}}),
            json!({"auth": {"type": "basic", "username": "u", "password": "P"}}),
        );
        assert!(c.is_empty());
        assert_eq!(
            v,
            json!({"auth": {"type": "basic", "username": "U", "password": "P"}})
        );
        let (_, c) = run(
            json!({"auth": {"type": "none"}}),
            json!({"auth": {"type": "bearer", "token": "a"}}),
            json!({"auth": {"type": "inherit"}}),
        );
        assert_eq!(c[0].field, vec!["auth"]);
    }

    #[test]
    fn secret_variables_never_show_values() {
        let var = |v: &str, s: bool| json!({"name": "k", "value": v, "enabled": true, "secret": s});
        let (_, c) = run(
            json!({"vars": [var("a", false)]}),
            json!({"vars": [var("b", true)]}),
            json!({"vars": [var("c", false)]}),
        );
        assert_eq!(
            c[0].values(),
            [
                Some(DiffValue::Secret),
                Some(DiffValue::Secret),
                Some(DiffValue::Secret)
            ]
        );
    }
}
