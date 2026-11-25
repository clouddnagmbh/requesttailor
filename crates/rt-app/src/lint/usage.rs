// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Which variable names a collection uses (wave 18, strand F, `orphan-variable`): `{{name}}`
//! in any string of the model or in a body file, names in assertions and actions, and literal
//! names in script calls (`rt.vars.get("name")`, `bru.getVar('name')`,
//! `pm.environment.get("name")`, ...). A script that reaches variables in a way that is not a
//! literal name (`rt.vars.get(name)`, `rt.vars.toObject()`, an alias of `rt`) makes the use
//! unknowable: [`Usage::dynamic`] is set and the rule reports nothing. Quieter is better than
//! wrong.

use std::collections::HashSet;

use rt_core::model::{Collection, Folder, Item, Script};

#[derive(Debug, Default)]
pub(super) struct Usage {
    pub names: HashSet<String>,
    /// A script reads or writes variables by a name the lint cannot see.
    pub dynamic: bool,
}

impl Usage {
    fn add(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.names.insert(name.to_owned());
        if let Some(rest) = name.strip_prefix("secret:") {
            self.add(rest);
        }
        if let Some((head, _)) = name.split_once(['.', '[']) {
            self.add(head);
        }
    }

    fn text(&mut self, text: &str) {
        let mut rest = text;
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else { break };
            let name = &after[..end];
            if !name.contains(['{', '}']) {
                self.add(name);
            }
            rest = &after[end + 2..];
        }
    }

    fn json(&mut self, value: &serde_json::Value) {
        match value {
            serde_json::Value::String(s) => self.text(s),
            serde_json::Value::Array(items) => items.iter().for_each(|v| self.json(v)),
            serde_json::Value::Object(map) => map.values().for_each(|v| self.json(v)),
            _ => {}
        }
    }

    fn scripts(&mut self, scripts: &[Script]) {
        for s in scripts {
            self.script(&s.code);
        }
    }

    fn folder(&mut self, folder: &Folder) {
        self.scripts(&folder.defaults.scripts);
        for item in &folder.items {
            self.item(item);
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Folder(f) => self.folder(f),
            Item::Request(r) => {
                self.scripts(&r.spec.scripts);
                for a in &r.spec.assertions {
                    self.add(&a.expression);
                }
                for a in &r.spec.actions {
                    self.add(&a.variable);
                    self.add(&a.expression);
                }
            }
        }
    }

    /// Looks at one script's code for variable access.
    fn script(&mut self, code: &str) {
        let mut found = Access::default();
        scan_script(code, &mut found);
        self.dynamic |= found.dynamic;
        for n in found.names {
            self.add(&n);
        }
    }
}

/// The names used in `collection` and `extra` (texts of body files).
pub(super) fn of(collection: &Collection, extra: &[&str]) -> Usage {
    let mut usage = Usage::default();
    if let Ok(value) = serde_json::to_value(collection) {
        usage.json(&value);
    }
    usage.scripts(&collection.defaults.scripts);
    for item in &collection.items {
        usage.item(item);
    }
    for text in extra {
        usage.text(text);
    }
    usage
}

#[derive(Default)]
struct Access {
    names: Vec<String>,
    dynamic: bool,
}

/// Objects whose variable methods take a name: `rt.vars`, `bru` (methods with `Var` in the
/// name), and Postman's and Insomnia's scopes.
const SCOPES: &[&str] = &[
    "rt.vars",
    "pm.variables",
    "pm.environment",
    "pm.collectionVariables",
    "pm.globals",
    "pm.iterationData",
    "insomnia.variables",
    "insomnia.environment",
    "insomnia.collectionVariables",
    "insomnia.globals",
];

/// Methods of a scope that take the variable's name first.
const NAMED: &[&str] = &["get", "set", "has", "unset"];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn scan_script(code: &str, out: &mut Access) {
    // Roots must not be aliased: any other use of `rt`, `bru`, `pm`, `insomnia` than
    // `root.` is unknowable, and `vars` taken apart from `rt.` too.
    let bytes: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if !is_ident(c) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_ident(bytes[i]) {
            i += 1;
        }
        let word: String = bytes[start..i].iter().collect();
        let before_dot = start > 0 && bytes[start - 1] == '.';
        if before_dot {
            continue;
        }
        if matches!(word.as_str(), "rt" | "bru" | "pm" | "insomnia") {
            let mut j = i;
            skip_space(&bytes, &mut j);
            if bytes.get(j) != Some(&'.') {
                // `const b = bru;`, `rt["vars"]`, `{ vars } = rt`.
                out.dynamic = true;
                continue;
            }
            // Read the dotted chain (`rt.vars.get`) and the call after it.
            let chain = chain(&bytes, start);
            access(&bytes, &chain, out);
        } else if word == "vars" {
            // `{ vars } = rt` and the like: `rt.vars` is handled with its root, the
            // word alone is an alias.
            out.dynamic = true;
        }
    }
}

struct Chain {
    /// `rt.vars.get`, without spaces.
    text: String,
    /// Index after the chain.
    end: usize,
}

fn skip_space(chars: &[char], i: &mut usize) {
    while chars.get(*i).is_some_and(|c| c.is_whitespace()) {
        *i += 1;
    }
}

fn chain(chars: &[char], start: usize) -> Chain {
    let mut text = String::new();
    let mut i = start;
    loop {
        let s = i;
        while i < chars.len() && is_ident(chars[i]) {
            i += 1;
        }
        text.extend(&chars[s..i]);
        let mut j = i;
        skip_space(chars, &mut j);
        if chars.get(j) == Some(&'.') {
            let mut k = j + 1;
            skip_space(chars, &mut k);
            if chars.get(k).is_some_and(|c| is_ident(*c)) {
                text.push('.');
                i = k;
                continue;
            }
        }
        break;
    }
    Chain { text, end: i }
}

fn access(chars: &[char], chain: &Chain, out: &mut Access) {
    let parts: Vec<&str> = chain.text.split('.').collect();
    // `bru.<method with Var>(...)`.
    let (scoped, method) = if parts.len() == 2 && parts[0] == "bru" {
        if !parts[1].contains("Var") && !parts[1].contains("var") && parts[1] != "interpolate" {
            return;
        }
        (true, parts[1])
    } else {
        let Some((method, scope_parts)) = parts.split_last() else {
            return;
        };
        let scope = scope_parts.join(".");
        if SCOPES.contains(&scope.as_str()) {
            (NAMED.contains(method), *method)
        } else if SCOPES.contains(&chain.text.as_str()) {
            // `rt.vars` alone: used as a value.
            out.dynamic = true;
            return;
        } else {
            // A prefix of a scope such as `pm.environment` followed by something else, or
            // an unrelated member (`rt.request`, `pm.response`).
            if SCOPES
                .iter()
                .any(|s| s.starts_with(&chain.text) || chain.text.starts_with(s))
            {
                out.dynamic = true;
            }
            return;
        }
    };
    if !scoped {
        // `rt.vars.toObject()`, `pm.variables.toObject()`, `replace`, ...: every variable or
        // a template; a literal template is read for `{{names}}` by the text scan.
        if method == "replace" || method == "interpolate" {
            match literal_arg(chars, chain.end) {
                Some(_) => {}
                None => out.dynamic = true,
            }
        } else {
            out.dynamic = true;
        }
        return;
    }
    if method == "interpolate" {
        if literal_arg(chars, chain.end).is_none() {
            out.dynamic = true;
        }
        return;
    }
    match literal_arg(chars, chain.end) {
        Some(name) => out.names.push(name),
        None => out.dynamic = true,
    }
}

/// The first argument of the call whose `(` follows `at`, if it is a plain string literal
/// (`'x'`, `"x"` or a template without `${`) followed by `,` or `)`.
fn literal_arg(chars: &[char], at: usize) -> Option<String> {
    let mut i = at;
    skip_space(chars, &mut i);
    if chars.get(i) != Some(&'(') {
        return None;
    }
    i += 1;
    skip_space(chars, &mut i);
    let quote = *chars.get(i)?;
    if !matches!(quote, '"' | '\'' | '`') {
        return None;
    }
    i += 1;
    let mut name = String::new();
    loop {
        let c = *chars.get(i)?;
        if c == quote {
            break;
        }
        if c == '\\' || (quote == '`' && c == '$' && chars.get(i + 1) == Some(&'{')) {
            return None;
        }
        name.push(c);
        i += 1;
    }
    i += 1;
    skip_space(chars, &mut i);
    matches!(chars.get(i), Some(',') | Some(')')).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(code: &str) -> Access {
        let mut a = Access::default();
        scan_script(code, &mut a);
        a
    }

    #[test]
    fn literal_names_are_read() {
        let a =
            scan("const t = rt.vars.get(\"token\"); bru.setVar('x', 1); pm.environment.get( `y` )");
        assert!(!a.dynamic);
        assert_eq!(a.names, vec!["token", "x", "y"]);
    }

    #[test]
    fn everything_else_is_dynamic() {
        for code in [
            "rt.vars.get(k)",
            "rt.vars.get('a' + k)",
            "rt.vars.toObject()",
            "const v = rt.vars",
            "const b = bru;",
            "const { vars } = rt;",
            "pm.variables.get(`${a}`)",
        ] {
            assert!(scan(code).dynamic, "{code}");
        }
    }

    #[test]
    fn unrelated_calls_are_not_variable_access() {
        let a = scan(
            "rt.request.setHeader('A', 'b'); expect(res.status).to.equal(200); test('x', () => {});",
        );
        assert!(!a.dynamic, "{:?}", a.names);
        assert!(a.names.is_empty());
    }
}
