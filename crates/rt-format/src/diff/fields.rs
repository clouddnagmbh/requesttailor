// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Turns model values into flat lists of fields (path, value) and compares two such lists.
//! The path table is in the module docs of [`super`].

use std::collections::{HashMap, HashSet};

use rt_core::model::{
    Auth, CollectionConfig, Environment, KeyValue, Param, ParamKind, RequestDefaults, RequestSpec,
    Script, ScriptPhase, Settings, Variable,
};
use serde::Serialize;

use super::{DiffValue, FieldChange, FieldPath};

/// Segment appended to the path of a list entry for its `enabled` flag.
pub(super) const ENABLED: &str = "@enabled";

/// One field of a model value.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Field {
    pub path: FieldPath,
    pub value: DiffValue,
    /// The field holds the value of a secret variable: it is masked in the output.
    pub secret: bool,
}

fn path(parts: &[&str]) -> FieldPath {
    parts.iter().map(|s| (*s).to_owned()).collect()
}

fn child(prefix: &[&str], parts: &[&str]) -> FieldPath {
    prefix
        .iter()
        .chain(parts)
        .map(|s| (*s).to_owned())
        .collect()
}

fn text(path: FieldPath, text: impl Into<String>) -> Field {
    Field {
        path,
        value: DiffValue::Text { text: text.into() },
        secret: false,
    }
}

fn json(path: FieldPath, value: serde_json::Value) -> Field {
    match value {
        serde_json::Value::String(s) => text(path, s),
        value => Field {
            path,
            value: DiffValue::Json { value },
            secret: false,
        },
    }
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// Gives repeated keys a `#2`, `#3`, ... suffix, so every list entry has its own path.
struct Unique(HashMap<String, usize>);

impl Unique {
    fn new() -> Self {
        Unique(HashMap::new())
    }
    fn key(&mut self, key: &str) -> String {
        let n = self.0.entry(key.to_owned()).or_insert(0);
        *n += 1;
        if *n == 1 {
            key.to_owned()
        } else {
            format!("{key}#{n}")
        }
    }
}

/// An entry of a list: its value and, as a second field, whether it is enabled.
fn entry(out: &mut Vec<Field>, prefix: &[&str], key: &str, value: Field, enabled: bool) {
    let mut p = child(prefix, &[key]);
    let mut v = value;
    v.path = p.clone();
    out.push(v);
    p.push(ENABLED.to_owned());
    out.push(text(p, enabled.to_string()));
}

/// The fields of a JSON object, one per key; strings are text, `null`s are absent.
fn flatten(out: &mut Vec<Field>, prefix: &[&str], value: &impl Serialize, skip: &[&str]) {
    let Ok(serde_json::Value::Object(map)) = serde_json::to_value(value) else {
        return;
    };
    for (k, v) in map {
        if v.is_null() || skip.contains(&k.as_str()) {
            continue;
        }
        out.push(json(child(prefix, &[&k]), v));
    }
}

fn headers(out: &mut Vec<Field>, prefix: &[&str], list: &[KeyValue]) {
    let mut u = Unique::new();
    let prefix: Vec<&str> = prefix.iter().copied().chain(["headers"]).collect();
    for h in list {
        let key = u.key(&h.name);
        entry(
            out,
            &prefix,
            &key,
            text(Vec::new(), h.value.clone()),
            h.enabled,
        );
    }
}

fn params(out: &mut Vec<Field>, list: &[Param]) {
    let mut u = Unique::new();
    for p in list {
        let kind = match p.kind {
            ParamKind::Query => "query",
            ParamKind::Path => "path",
        };
        let key = u.key(&format!("{kind}\u{0}{}", p.name));
        let name = key.split_once('\u{0}').map_or(key.as_str(), |(_, n)| n);
        entry(
            out,
            &["params", kind],
            name,
            text(Vec::new(), p.value.clone()),
            p.enabled,
        );
    }
}

fn vars(out: &mut Vec<Field>, prefix: &[&str], list: &[Variable]) {
    let mut u = Unique::new();
    let prefix: Vec<&str> = prefix.iter().copied().chain(["vars"]).collect();
    for v in list {
        let key = u.key(&v.name);
        let mut f = text(Vec::new(), v.value.clone().unwrap_or_default());
        f.secret = v.secret;
        entry(out, &prefix, &key, f, v.enabled);
        if v.secret {
            // The flag itself is a change worth showing, the value is not.
            let mut p = child(&prefix, &[&key]);
            p.push("@secret".into());
            out.push(text(p, "true"));
        }
    }
}

fn settings(out: &mut Vec<Field>, prefix: &[&str], s: &Settings) {
    let prefix: Vec<&str> = prefix.iter().copied().chain(["settings"]).collect();
    flatten(out, &prefix, s, &[]);
}

fn auth(out: &mut Vec<Field>, prefix: &[&str], a: &Auth) {
    if *a == Auth::Inherit {
        return;
    }
    let prefix: Vec<&str> = prefix.iter().copied().chain(["auth"]).collect();
    flatten(out, &prefix, a, &[]);
}

fn scripts(out: &mut Vec<Field>, prefix: &[&str], list: &[Script]) {
    let mut u = Unique::new();
    for s in list {
        let phase = match s.phase {
            ScriptPhase::BeforeRequest => "before-request",
            ScriptPhase::AfterResponse => "after-response",
            ScriptPhase::Tests => "tests",
        };
        let key = u.key(phase);
        out.push(text(child(prefix, &["scripts", &key]), s.code.clone()));
    }
}

/// Headers, auth, variables, settings and scripts shared by folders and the collection.
pub(super) fn defaults(d: &RequestDefaults) -> Vec<Field> {
    let mut out = Vec::new();
    let p = ["defaults"];
    headers(&mut out, &p, &d.headers);
    auth(&mut out, &p, &d.auth);
    vars(&mut out, &p, &d.vars);
    settings(&mut out, &p, &d.settings);
    scripts(&mut out, &p, &d.scripts);
    out
}

pub(super) fn folder(name: &str, seq: Option<f64>, d: &RequestDefaults) -> Vec<Field> {
    let mut out = vec![text(path(&["name"]), name)];
    if let Some(s) = seq {
        out.push(text(path(&["seq"]), number(s)));
    }
    out.extend(defaults(d));
    out
}

pub(super) fn collection(name: &str, d: &RequestDefaults, c: &CollectionConfig) -> Vec<Field> {
    let mut out = vec![text(path(&["name"]), name)];
    out.extend(defaults(d));
    flatten(&mut out, &["config"], c, &[]);
    out
}

pub(super) fn environment(e: &Environment) -> Vec<Field> {
    let mut out = Vec::new();
    vars(&mut out, &[], &e.variables);
    // Environment variables live under `variables`, not `vars`.
    for f in &mut out {
        f.path[0] = "variables".into();
    }
    if !e.client_certificates.is_empty()
        && let Ok(v) = serde_json::to_value(&e.client_certificates)
    {
        out.push(json(path(&["client_certificates"]), v));
    }
    if let Some(s) = &e.external_secrets
        && let Ok(v) = serde_json::to_value(s)
    {
        out.push(json(path(&["external_secrets"]), v));
    }
    out
}

pub(super) fn request(s: &RequestSpec) -> Vec<Field> {
    let mut out = vec![text(path(&["name"]), s.name.clone())];
    if let Some(n) = s.seq {
        out.push(text(path(&["seq"]), number(n)));
    }
    out.push(text(path(&["method"]), s.method.clone()));
    out.push(text(path(&["url"]), s.url.clone()));
    headers(&mut out, &[], &s.headers);
    params(&mut out, &s.params);
    if s.body != rt_core::model::Body::None {
        flatten(&mut out, &["body"], &s.body, &[]);
    }
    auth(&mut out, &[], &s.auth);
    vars(&mut out, &[], &s.vars);
    settings(&mut out, &[], &s.settings);
    scripts(&mut out, &[], &s.scripts);
    let mut u = Unique::new();
    for a in &s.assertions {
        let key = u.key(&a.expression);
        let mut v = serde_json::to_value(a).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("expression");
        }
        out.push(json(child(&["assertions"], &[&key]), v));
    }
    let mut u = Unique::new();
    for a in &s.actions {
        let key = u.key(&a.variable);
        let mut v = serde_json::to_value(a).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("variable");
        }
        out.push(json(child(&["actions"], &[&key]), v));
    }
    protocol(&mut out, &s.protocol);
    out
}

fn protocol(out: &mut Vec<Field>, p: &rt_core::model::Protocol) {
    use rt_core::model::{MessageVariant, Protocol};
    let messages: &[MessageVariant] = match p {
        Protocol::Http => return,
        Protocol::Grpc(g) => &g.messages,
        Protocol::WebSocket(w) => &w.messages,
    };
    flatten(out, &["protocol"], p, &["messages"]);
    let mut u = Unique::new();
    for (i, m) in messages.iter().enumerate() {
        let key = if m.title.is_empty() {
            format!("#{}", i + 1)
        } else {
            m.title.clone()
        };
        let key = u.key(&key);
        out.push(text(
            child(&["protocol", "messages"], &[&key]),
            m.data.clone(),
        ));
        let kind = serde_json::to_value(m.kind).unwrap_or_default();
        out.push(json(
            child(&["protocol", "messages"], &[&key, "kind"]),
            kind,
        ));
        out.push(text(
            child(&["protocol", "messages"], &[&key, "selected"]),
            m.selected.to_string(),
        ));
    }
}

/// Order of the first path segment in the output.
fn rank(first: &str) -> usize {
    const ORDER: &[&str] = &[
        "name",
        "seq",
        "method",
        "url",
        "headers",
        "params",
        "body",
        "auth",
        "vars",
        "settings",
        "scripts",
        "assertions",
        "actions",
        "protocol",
        "variables",
        "defaults",
        "config",
    ];
    ORDER
        .iter()
        .position(|s| *s == first)
        .unwrap_or(ORDER.len())
}

fn masked(f: &Field, hide: bool) -> DiffValue {
    if hide {
        DiffValue::Secret
    } else {
        f.value.clone()
    }
}

/// The differences between two field lists, in model order. A secret on either side hides
/// the values on both. The `enabled` flag of an entry that was added or removed whole is not
/// a change of its own while it is `true`.
pub(super) fn compare(old: &[Field], new: &[Field]) -> Vec<FieldChange> {
    let new_at: HashMap<&FieldPath, &Field> = new.iter().map(|f| (&f.path, f)).collect();
    let old_at: HashMap<&FieldPath, &Field> = old.iter().map(|f| (&f.path, f)).collect();
    let mut out: Vec<FieldChange> = Vec::new();
    for o in old {
        match new_at.get(&o.path) {
            Some(n) if n.value == o.value && n.secret == o.secret => {}
            Some(n) => {
                let hide = o.secret || n.secret;
                out.push(FieldChange {
                    path: o.path.clone(),
                    old: Some(masked(o, hide)),
                    new: Some(masked(n, hide)),
                    line: None,
                });
            }
            None => out.push(FieldChange {
                path: o.path.clone(),
                old: Some(masked(o, o.secret)),
                new: None,
                line: None,
            }),
        }
    }
    for n in new {
        if !old_at.contains_key(&n.path) {
            out.push(FieldChange {
                path: n.path.clone(),
                old: None,
                new: Some(masked(n, n.secret)),
                line: None,
            });
        }
    }
    let whole: HashSet<FieldPath> = out
        .iter()
        .filter(|c| c.old.is_none() || c.new.is_none())
        .map(|c| c.path.clone())
        .collect();
    let is_true =
        |v: &Option<DiffValue>| matches!(v, Some(DiffValue::Text { text }) if text == "true");
    out.retain(|c| {
        let Some((last, parent)) = c.path.split_last() else {
            return true;
        };
        let flag = last == ENABLED || last == "@secret";
        if !flag || !whole.contains(parent) {
            return true;
        }
        // Dropped only when the flag says nothing special: enabled, and the usual
        // "not secret" of a removed secret.
        !(is_true(&c.new) || is_true(&c.old))
    });
    out.sort_by_key(|c| rank(&c.path[0]));
    out
}
