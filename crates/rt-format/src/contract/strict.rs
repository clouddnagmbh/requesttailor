// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Strict JSON Schema checks for the run wizard and the response check: everything `schema`
//! checks, plus `minLength`/`maxLength`, `minimum`/`maximum` (with `exclusive*` in the
//! OpenAPI 3.0 boolean and the 3.1 numeric form), `multipleOf`, `pattern`, `format` (date,
//! date-time, time, uuid, email, uri, ipv4, ipv6), `minItems`/`maxItems`, `uniqueItems`,
//! `const`. Also: schemas with local `$ref`s inlined for the UI, examples generated from a
//! schema, and parameter text coerced to the schema's type.
//!
//! The contract check (`check`) keeps its own, more lenient validation and messages.

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::schema::resolve;

const MAX_DEPTH: usize = 32;

/// One place where an instance does not fit its schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Violation {
    /// `$`, `$.items[0].id`.
    pub path: String,
    /// e.g. `integer`, `one of a, b`, `at most 10 characters`.
    pub expected: String,
    /// e.g. `string "x"`, `missing`.
    pub got: String,
    /// A plain sentence without the path: `must be an integer`, `required property is
    /// missing`.
    pub text: String,
}

impl Violation {
    /// `$.name: must be an integer`; just the text at the root.
    pub fn detail(&self) -> String {
        if self.path == "$" {
            self.text.clone()
        } else {
            format!("{}: {}", self.path, self.text)
        }
    }
}

/// The schema keywords of a schema and its `allOf` parts.
#[derive(Default)]
struct Merged<'a> {
    types: Vec<&'a str>,
    nullable: bool,
    format: Option<&'a str>,
    props: Vec<(&'a str, &'a Value)>,
    required: Vec<&'a str>,
    items: Option<&'a Value>,
    enums: Option<&'a [Value]>,
    konst: Option<&'a Value>,
    /// `additionalProperties`: `false` or a schema.
    additional: Option<&'a Value>,
    /// `oneOf`/`anyOf` groups: one alternative of each must fit.
    alternatives: Vec<&'a [Value]>,
    minimum: Option<(f64, bool)>,
    maximum: Option<(f64, bool)>,
    multiple_of: Option<f64>,
    min_length: Option<u64>,
    max_length: Option<u64>,
    patterns: Vec<&'a str>,
    min_items: Option<u64>,
    max_items: Option<u64>,
    unique: bool,
}

fn merged<'a>(root: &'a Value, schema: &'a Value) -> Merged<'a> {
    let mut out = Merged::default();
    merge(root, schema, &mut out, 0);
    out
}

fn merge<'a>(root: &'a Value, schema: &'a Value, out: &mut Merged<'a>, depth: usize) {
    let schema = resolve(root, schema);
    let Some(map) = schema.as_object() else {
        return;
    };
    if depth > 8 {
        return;
    }
    match map.get("type") {
        Some(Value::String(t)) => push_type(&mut out.types, t),
        Some(Value::Array(ts)) => {
            for t in ts.iter().filter_map(Value::as_str) {
                push_type(&mut out.types, t);
            }
        }
        _ => {}
    }
    if map.get("nullable") == Some(&Value::Bool(true)) {
        out.nullable = true;
    }
    if let Some(Value::String(f)) = map.get("format") {
        out.format = Some(f);
    }
    if let Some(Value::Object(props)) = map.get("properties") {
        for (k, v) in props {
            match out.props.iter_mut().find(|(n, _)| n == k) {
                Some(slot) => slot.1 = v,
                None => out.props.push((k, v)),
            }
        }
    }
    if let Some(Value::Array(req)) = map.get("required") {
        for r in req.iter().filter_map(Value::as_str) {
            if !out.required.contains(&r) {
                out.required.push(r);
            }
        }
    }
    if let Some(items) = map.get("items")
        && items.is_object()
    {
        out.items = Some(items);
    }
    if let Some(Value::Array(e)) = map.get("enum") {
        out.enums = Some(e);
    }
    if let Some(c) = map.get("const") {
        out.konst = Some(c);
    }
    if let Some(a) = map.get("additionalProperties")
        && (a == &Value::Bool(false) || a.is_object())
    {
        out.additional = Some(a);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(alts)) = map.get(key)
            && !alts.is_empty()
        {
            out.alternatives.push(alts);
        }
    }
    let num = |k: &str| map.get(k).and_then(Value::as_f64);
    if let Some(m) = num("minimum") {
        let exclusive = map.get("exclusiveMinimum") == Some(&Value::Bool(true));
        tighten(&mut out.minimum, m, exclusive, true);
    }
    if let Some(m) = num("exclusiveMinimum") {
        tighten(&mut out.minimum, m, true, true);
    }
    if let Some(m) = num("maximum") {
        let exclusive = map.get("exclusiveMaximum") == Some(&Value::Bool(true));
        tighten(&mut out.maximum, m, exclusive, false);
    }
    if let Some(m) = num("exclusiveMaximum") {
        tighten(&mut out.maximum, m, true, false);
    }
    if let Some(m) = num("multipleOf")
        && m > 0.0
    {
        out.multiple_of = Some(m);
    }
    let int = |k: &str| map.get(k).and_then(Value::as_u64);
    if let Some(n) = int("minLength") {
        out.min_length = Some(out.min_length.map_or(n, |o| o.max(n)));
    }
    if let Some(n) = int("maxLength") {
        out.max_length = Some(out.max_length.map_or(n, |o| o.min(n)));
    }
    if let Some(Value::String(p)) = map.get("pattern") {
        out.patterns.push(p);
    }
    if let Some(n) = int("minItems") {
        out.min_items = Some(out.min_items.map_or(n, |o| o.max(n)));
    }
    if let Some(n) = int("maxItems") {
        out.max_items = Some(out.max_items.map_or(n, |o| o.min(n)));
    }
    if map.get("uniqueItems") == Some(&Value::Bool(true)) {
        out.unique = true;
    }
    if let Some(Value::Array(all)) = map.get("allOf") {
        for part in all {
            merge(root, part, out, depth + 1);
        }
    }
}

fn push_type<'a>(types: &mut Vec<&'a str>, t: &'a str) {
    if !types.contains(&t) {
        types.push(t);
    }
}

/// Keeps the stricter bound.
fn tighten(slot: &mut Option<(f64, bool)>, value: f64, exclusive: bool, lower: bool) {
    let stricter = match *slot {
        None => true,
        Some((v, e)) => {
            if lower {
                value > v || (value == v && exclusive && !e)
            } else {
                value < v || (value == v && exclusive && !e)
            }
        }
    };
    if stricter {
        *slot = Some((value, exclusive));
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

fn article(word: &str) -> &'static str {
    let lower = word.to_ascii_lowercase();
    if ["uuid", "uri", "url", "uni"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return "a";
    }
    match lower.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// `string "x"`, `integer 5`, `array of 3 items`, `object`.
pub(super) fn describe(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => format!("boolean {b}"),
        Value::Number(n) => format!("{} {n}", kind_of(v)),
        Value::String(s) => format!("string {}", quote(s)),
        Value::Array(a) => match a.len() {
            1 => "array of 1 item".into(),
            n => format!("array of {n} items"),
        },
        Value::Object(_) => "object".into(),
    }
}

fn quote(s: &str) -> String {
    const MAX: usize = 40;
    if s.chars().count() > MAX {
        let cut: String = s.chars().take(MAX).collect();
        format!("\"{cut}…\"")
    } else {
        format!("\"{s}\"")
    }
}

/// An enum or const value as the user reads it: strings bare, the rest as JSON.
fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// Validates `inst` against `schema` (local `$ref`s against `root`), at most `limit`
/// violations. Strings with `{{templates}}` are not judged.
pub(super) fn validate<'a>(
    root: &'a Value,
    schema: &'a Value,
    inst: &Value,
    limit: usize,
) -> Vec<Violation> {
    let mut w = Walker {
        root,
        limit,
        out: Vec::new(),
        regexes: HashMap::new(),
    };
    let mut path = String::from("$");
    w.walk(schema, inst, &mut path, 0);
    w.out
}

struct Walker<'a> {
    root: &'a Value,
    limit: usize,
    out: Vec<Violation>,
    regexes: HashMap<String, Option<regex_lite::Regex>>,
}

impl<'a> Walker<'a> {
    fn push(&mut self, path: &str, expected: String, got: String, text: String) {
        if self.out.len() < self.limit {
            self.out.push(Violation {
                path: path.to_owned(),
                expected,
                got,
                text,
            });
        }
    }

    fn full(&self) -> bool {
        self.out.len() >= self.limit
    }

    fn walk(&mut self, schema: &'a Value, inst: &Value, path: &mut String, depth: usize) {
        if self.full() || depth > MAX_DEPTH {
            return;
        }
        if let Value::String(s) = inst
            && s.contains("{{")
        {
            return;
        }
        let m = merged(self.root, schema);
        let actual = kind_of(inst);
        if actual == "null" && m.nullable {
            return;
        }
        let fits = m.types.is_empty()
            || m.types.contains(&actual)
            || (actual == "integer" && m.types.contains(&"number"));
        if !fits {
            let mut want: Vec<&str> = m.types.clone();
            if m.nullable && !want.contains(&"null") {
                want.push("null");
            }
            let expected = want.join(" or ");
            let text = format!("must be {} {expected}", article(&expected));
            self.push(path, expected, describe(inst), text);
            return;
        }
        if let Some(enums) = m.enums
            && !enums.iter().any(|e| same(e, inst))
        {
            let list: Vec<String> = enums.iter().map(plain).collect();
            let expected = format!("one of {}", list.join(", "));
            let text = format!("must be {expected}");
            self.push(path, expected, describe(inst), text);
        }
        if let Some(c) = m.konst
            && !same(c, inst)
        {
            let expected = format!("exactly {}", plain(c));
            let text = format!("must be {expected}");
            self.push(path, expected, describe(inst), text);
        }
        for group in &m.alternatives {
            let fits = group.iter().any(|alt| {
                let mut sub = Walker {
                    root: self.root,
                    limit: 1,
                    out: Vec::new(),
                    regexes: std::mem::take(&mut self.regexes),
                };
                let mut p = String::from("$");
                sub.walk(alt, inst, &mut p, depth + 1);
                self.regexes = sub.regexes;
                sub.out.is_empty()
            });
            if !fits {
                self.push(
                    path,
                    "one of the allowed shapes".into(),
                    describe(inst),
                    "matches none of the allowed shapes".into(),
                );
            }
        }
        match inst {
            Value::String(s) => self.string(&m, s, inst, path),
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    self.number(&m, f, inst, path);
                }
            }
            Value::Array(list) => {
                let n = list.len() as u64;
                if let Some(min) = m.min_items
                    && n < min
                {
                    let expected = format!("at least {min} item{}", plural(min));
                    let text = format!("must have {expected}");
                    self.push(path, expected, format!("{n} item{}", plural(n)), text);
                }
                if let Some(max) = m.max_items
                    && n > max
                {
                    let expected = format!("at most {max} item{}", plural(max));
                    let text = format!("must have {expected}");
                    self.push(path, expected, format!("{n} item{}", plural(n)), text);
                }
                if m.unique && list.len() > 1 {
                    let mut seen = std::collections::HashSet::new();
                    if let Some(dup) = list.iter().find(|v| !seen.insert(v.to_string())) {
                        self.push(
                            path,
                            "unique items".into(),
                            format!("{} twice", describe(dup)),
                            "must not contain the same item twice".into(),
                        );
                    }
                }
                if let Some(items) = m.items {
                    let len = path.len();
                    for (i, v) in list.iter().enumerate() {
                        if self.full() {
                            break;
                        }
                        use std::fmt::Write as _;
                        let _ = write!(path, "[{i}]");
                        self.walk(items, v, path, depth + 1);
                        path.truncate(len);
                    }
                }
            }
            Value::Object(map) => {
                let len = path.len();
                for name in &m.required {
                    if !map.contains_key(*name) {
                        push_key(path, name);
                        let p = path.clone();
                        path.truncate(len);
                        self.push(
                            &p,
                            "required property".into(),
                            "missing".into(),
                            "required property is missing".into(),
                        );
                    }
                }
                for (k, v) in map {
                    if self.full() {
                        break;
                    }
                    push_key(path, k);
                    if let Some((_, s)) = m.props.iter().find(|(n, _)| n == k) {
                        self.walk(s, v, path, depth + 1);
                    } else if let Some(extra) = m.additional {
                        if extra.is_object() {
                            self.walk(extra, v, path, depth + 1);
                        } else {
                            let p = path.clone();
                            self.push(
                                &p,
                                "no such property".into(),
                                describe(v),
                                "property is not in the schema".into(),
                            );
                        }
                    }
                    path.truncate(len);
                }
            }
            _ => {}
        }
    }

    fn string(&mut self, m: &Merged<'a>, s: &str, inst: &Value, path: &str) {
        if m.min_length.is_some() || m.max_length.is_some() {
            let n = s.chars().count() as u64;
            if let Some(min) = m.min_length
                && n < min
            {
                let expected = format!("at least {min} character{}", plural(min));
                let text = format!("must be {expected}");
                self.push(path, expected, format!("{n} character{}", plural(n)), text);
            }
            if let Some(max) = m.max_length
                && n > max
            {
                let expected = format!("at most {max} character{}", plural(max));
                let text = format!("must be {expected}");
                self.push(path, expected, format!("{n} character{}", plural(n)), text);
            }
        }
        for pattern in &m.patterns {
            let re = self
                .regexes
                .entry((*pattern).to_owned())
                .or_insert_with(|| regex_lite::Regex::new(pattern).ok());
            // An invalid pattern is ignored.
            if let Some(re) = re
                && !re.is_match(s)
            {
                let expected = format!("matching {pattern}");
                let text = format!("must match {pattern}");
                self.push(path, expected, describe(inst), text);
            }
        }
        if let Some(format) = m.format
            && let Some((noun, ok)) = check_format(format, s)
            && !ok
        {
            let text = format!("must be {} {noun}", article(noun));
            self.push(path, noun.to_owned(), describe(inst), text);
        }
    }

    fn number(&mut self, m: &Merged<'a>, f: f64, inst: &Value, path: &str) {
        if let Some((min, exclusive)) = m.minimum {
            let bad = if exclusive { f <= min } else { f < min };
            if bad {
                let expected = if exclusive {
                    format!("greater than {}", number(min))
                } else {
                    format!("at least {}", number(min))
                };
                let text = format!("must be {expected}");
                self.push(path, expected, describe(inst), text);
            }
        }
        if let Some((max, exclusive)) = m.maximum {
            let bad = if exclusive { f >= max } else { f > max };
            if bad {
                let expected = if exclusive {
                    format!("less than {}", number(max))
                } else {
                    format!("at most {}", number(max))
                };
                let text = format!("must be {expected}");
                self.push(path, expected, describe(inst), text);
            }
        }
        if let Some(step) = m.multiple_of {
            let q = f / step;
            if (q - q.round()).abs() > 1e-9 {
                let expected = format!("a multiple of {}", number(step));
                let text = format!("must be {expected}");
                self.push(path, expected, describe(inst), text);
            }
        }
    }
}

fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `.name`, or `["odd name"]` for keys that are not identifiers.
fn push_key(path: &mut String, key: &str) {
    let ident = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '-');
    if ident {
        path.push('.');
        path.push_str(key);
    } else {
        path.push_str("[\"");
        path.push_str(&key.replace('"', "\\\""));
        path.push_str("\"]");
    }
}

/// JSON equality, with `1` and `1.0` equal.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// The noun of a known format and whether `s` has it; `None` for formats not checked.
fn check_format(format: &str, s: &str) -> Option<(&'static str, bool)> {
    Some(match format {
        "date" => ("date (YYYY-MM-DD)", is_date(s)),
        "date-time" => ("date-time (e.g. 2024-01-31T12:00:00Z)", is_date_time(s)),
        "time" => ("time (hh:mm:ss)", is_time(s, false)),
        "uuid" => ("UUID", is_uuid(s)),
        "email" => ("email address", is_email(s)),
        "uri" | "url" => ("URI", is_uri(s)),
        "ipv4" => ("IPv4 address", s.parse::<std::net::Ipv4Addr>().is_ok()),
        "ipv6" => ("IPv6 address", s.parse::<std::net::Ipv6Addr>().is_ok()),
        _ => return None,
    })
}

fn digits(s: &str, n: usize) -> Option<u32> {
    (s.len() == n && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())?
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let (Some(y), Some(m), Some(d)) = (digits(&s[..4], 4), digits(&s[5..7], 2), digits(&s[8..], 2))
    else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// `hh:mm:ss[.frac]` and, if `offset` is required (date-time) or present, `Z` or `±hh:mm`.
fn is_time(s: &str, offset: bool) -> bool {
    if s.len() < 8 || !s.is_char_boundary(8) {
        return false;
    }
    let (hms, mut rest) = s.split_at(8);
    let b = hms.as_bytes();
    if b[2] != b':' || b[5] != b':' {
        return false;
    }
    let (Some(h), Some(m), Some(sec)) = (
        digits(&hms[..2], 2),
        digits(&hms[3..5], 2),
        digits(&hms[6..], 2),
    ) else {
        return false;
    };
    if h > 23 || m > 59 || sec > 60 {
        return false;
    }
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return false;
        }
        rest = &frac[n..];
    }
    match rest {
        "" => !offset,
        "Z" | "z" => true,
        _ => {
            let r = rest.as_bytes();
            r.len() == 6
                && (r[0] == b'+' || r[0] == b'-')
                && r[3] == b':'
                && digits(&rest[1..3], 2).is_some_and(|h| h <= 23)
                && digits(&rest[4..], 2).is_some_and(|m| m <= 59)
        }
    }
}

fn is_date_time(s: &str) -> bool {
    s.len() > 11
        && s.is_char_boundary(10)
        && is_date(&s[..10])
        && matches!(s.as_bytes()[10], b'T' | b't' | b' ')
        && is_time(&s[11..], true)
}

fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !local.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn is_uri(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        && !rest.is_empty()
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

// ---- schemas for the UI ----

const MAX_INLINE_REFS: usize = 6;
const MAX_INLINE_NODES: usize = 4000;

/// `schema` with local `$ref`s replaced by what they point to: at most a few nested
/// references deep, a cycle (or anything deeper) as `{}`.
pub(super) fn inline(root: &Value, schema: &Value) -> Value {
    let mut stack: Vec<&str> = Vec::new();
    let mut budget = MAX_INLINE_NODES;
    inline_in(root, schema, &mut stack, &mut budget, 0)
}

fn inline_in<'a>(
    root: &'a Value,
    v: &'a Value,
    stack: &mut Vec<&'a str>,
    budget: &mut usize,
    depth: usize,
) -> Value {
    if *budget == 0 || depth > MAX_DEPTH {
        return Value::Object(Map::new());
    }
    *budget -= 1;
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref")
                && let Some(ptr) = r.strip_prefix('#')
            {
                if stack.contains(&r.as_str()) || stack.len() >= MAX_INLINE_REFS {
                    return Value::Object(Map::new());
                }
                let Some(target) = root.pointer(ptr) else {
                    return v.clone();
                };
                stack.push(r);
                let mut out = inline_in(root, target, stack, budget, depth + 1);
                stack.pop();
                // OpenAPI 3.1 allows keywords next to `$ref`.
                if let Value::Object(o) = &mut out {
                    for (k, x) in map {
                        if k != "$ref" {
                            let x = inline_in(root, x, stack, budget, depth + 1);
                            o.insert(k.clone(), x);
                        }
                    }
                }
                return out;
            }
            Value::Object(
                map.iter()
                    .map(|(k, x)| {
                        // `example`, `default` and `enum` are values, not schemas.
                        let x = if matches!(
                            k.as_str(),
                            "example" | "examples" | "default" | "enum" | "const"
                        ) {
                            x.clone()
                        } else {
                            inline_in(root, x, stack, budget, depth + 1)
                        };
                        (k.clone(), x)
                    })
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|x| inline_in(root, x, stack, budget, depth + 1))
                .collect(),
        ),
        other => other.clone(),
    }
}

// ---- examples from a schema ----

const MAX_SAMPLE_DEPTH: usize = 6;
const MAX_SAMPLE_PROPS: usize = 40;

/// The schema's own example, else one generated from it (bounded).
pub(super) fn sample(root: &Value, schema: &Value) -> Value {
    let mut stack = Vec::new();
    sample_in(root, schema, &mut stack, 0)
}

/// An explicit example of the schema itself: `example`, `examples[0]`, `default`, `const`,
/// `enum[0]`.
pub(super) fn own_example(root: &Value, schema: &Value) -> Option<Value> {
    let s = resolve(root, schema);
    if let Some(e) = s.get("example") {
        return Some(e.clone());
    }
    if let Some(Value::Array(list)) = s.get("examples")
        && let Some(first) = list.first()
    {
        return Some(first.clone());
    }
    if let Some(d) = s.get("default") {
        return Some(d.clone());
    }
    if let Some(c) = s.get("const") {
        return Some(c.clone());
    }
    if let Some(Value::Array(e)) = s.get("enum")
        && let Some(first) = e.iter().find(|v| !v.is_null())
    {
        return Some(first.clone());
    }
    None
}

fn sample_in<'a>(
    root: &'a Value,
    schema: &'a Value,
    stack: &mut Vec<&'a str>,
    depth: usize,
) -> Value {
    if let Some(Value::String(r)) = schema.get("$ref") {
        if stack.contains(&r.as_str()) || depth > MAX_SAMPLE_DEPTH {
            return Value::Null;
        }
        stack.push(r);
        let out = sample_in(root, resolve(root, schema), stack, depth + 1);
        stack.pop();
        return out;
    }
    if depth > MAX_SAMPLE_DEPTH || !schema.is_object() {
        return Value::Null;
    }
    if let Some(e) = own_example(root, schema) {
        return e;
    }
    if let Some(Value::Array(all)) = schema.get("allOf") {
        let mut obj = Map::new();
        let mut other = None;
        for part in all {
            match sample_in(root, part, stack, depth + 1) {
                Value::Object(o) => obj.extend(o),
                Value::Null => {}
                v => other = Some(v),
            }
        }
        let own = sample_props(root, schema, stack, depth);
        obj.extend(own);
        return match other {
            Some(v) if obj.is_empty() => v,
            _ => Value::Object(obj),
        };
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(alts)) = schema.get(key)
            && let Some(first) = alts.first()
        {
            return sample_in(root, first, stack, depth + 1);
        }
    }
    let ty = match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .unwrap_or("null"),
        _ if schema.get("properties").is_some() => "object",
        _ if schema.get("items").is_some() => "array",
        _ => "",
    };
    match ty {
        "object" => Value::Object(sample_props(root, schema, stack, depth)),
        "array" => match schema.get("items") {
            Some(items) => {
                let item = sample_in(root, items, stack, depth + 1);
                if item.is_null() {
                    Value::Array(Vec::new())
                } else {
                    Value::Array(vec![item])
                }
            }
            None => Value::Array(Vec::new()),
        },
        "string" => Value::String(
            match schema.get("format").and_then(Value::as_str) {
                Some("date") => "2024-01-31",
                Some("date-time") => "2024-01-31T12:00:00Z",
                Some("time") => "12:00:00",
                Some("uuid") => "3fa85f64-5717-4562-b3fc-2c963f66afa6",
                Some("email") => "user@example.com",
                Some("uri" | "url") => "https://example.com",
                Some("ipv4") => "192.0.2.1",
                Some("ipv6") => "2001:db8::1",
                Some("byte") => "U3dhZ2dlcg==",
                _ => "string",
            }
            .to_owned(),
        ),
        "integer" => {
            let min = schema.get("minimum").and_then(Value::as_f64);
            let max = schema.get("maximum").and_then(Value::as_f64);
            let v = match (min, max) {
                (Some(m), _) => m.ceil(),
                (None, Some(m)) if m < 0.0 => m.floor(),
                _ => 0.0,
            };
            Value::from(v as i64)
        }
        "number" => {
            let v = schema.get("minimum").and_then(Value::as_f64).unwrap_or(0.0);
            serde_json::Number::from_f64(v).map_or(Value::from(0), Value::Number)
        }
        "boolean" => Value::Bool(true),
        _ => Value::Null,
    }
}

fn sample_props<'a>(
    root: &'a Value,
    schema: &'a Value,
    stack: &mut Vec<&'a str>,
    depth: usize,
) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(Value::Object(props)) = schema.get("properties") {
        for (k, v) in props.iter().take(MAX_SAMPLE_PROPS) {
            let r = resolve(root, v);
            if r.get("readOnly") == Some(&Value::Bool(true)) {
                continue;
            }
            out.insert(k.clone(), sample_in(root, v, stack, depth + 1));
        }
    }
    out
}

// ---- parameter text ----

/// How the values of one parameter are written (OpenAPI `style`/`explode`).
pub(super) struct Style<'a> {
    pub style: Option<&'a str>,
    pub explode: Option<bool>,
}

/// The value of a parameter as text (each occurrence, already decoded), as the JSON value its
/// schema describes: numbers, booleans, arrays (comma-, space- or pipe-separated, or one
/// occurrence per item). Text that does not parse stays a string, so validation reports the
/// type.
pub(super) fn coerce(root: &Value, schema: &Value, values: &[&str], style: &Style<'_>) -> Value {
    let m = merged(root, schema);
    if m.types.contains(&"array") {
        let parts: Vec<&str> = if values.len() > 1 {
            values.to_vec()
        } else {
            let one = values.first().copied().unwrap_or("");
            let sep = match style.style {
                Some("spaceDelimited") => ' ',
                Some("pipeDelimited") => '|',
                Some("tabDelimited") => '\t',
                _ => ',',
            };
            if one.is_empty() {
                Vec::new()
            } else {
                one.split(sep).collect()
            }
        };
        let item = m.items.unwrap_or(&Value::Null);
        return Value::Array(parts.iter().map(|p| scalar(root, item, p)).collect());
    }
    let _ = style.explode;
    scalar(root, schema, values.first().copied().unwrap_or(""))
}

fn scalar(root: &Value, schema: &Value, text: &str) -> Value {
    let m = merged(root, schema);
    let t = text.trim();
    if m.types.contains(&"integer")
        && let Ok(i) = t.parse::<i64>()
    {
        return Value::from(i);
    }
    if m.types.contains(&"number")
        && let Ok(f) = t.parse::<f64>()
        && let Some(n) = serde_json::Number::from_f64(f)
    {
        return Value::Number(n);
    }
    if m.types.contains(&"boolean") {
        match t {
            "true" => return Value::Bool(true),
            "false" => return Value::Bool(false),
            _ => {}
        }
    }
    if (m.types.contains(&"null") || m.nullable) && t == "null" {
        return Value::Null;
    }
    // Enums of numbers without a type.
    if m.types.is_empty()
        && let Some(enums) = m.enums
        && enums.iter().any(Value::is_number)
        && let Ok(f) = t.parse::<f64>()
        && let Some(n) = serde_json::Number::from_f64(f)
    {
        return Value::Number(n);
    }
    Value::String(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert!(is_date("2024-02-29"));
        assert!(!is_date("2023-02-29"));
        assert!(!is_date("2024-13-01"));
        assert!(is_date_time("2024-01-31T12:00:00Z"));
        assert!(is_date_time("2024-01-31T12:00:00.123+02:00"));
        assert!(!is_date_time("2024-01-31T12:00:00"));
        assert!(is_time("12:00:00", false));
        assert!(!is_time("25:00:00", false));
        assert!(is_uuid("3fa85f64-5717-4562-b3fc-2c963f66afa6"));
        assert!(!is_uuid("3fa85f64"));
        assert!(is_email("a@b.co"));
        assert!(!is_email("a@b"));
        assert!(is_uri("https://x.y/z"));
        assert!(!is_uri("not a uri"));
    }

    #[test]
    fn inline_cycles() {
        let root = serde_json::json!({
            "components": { "schemas": {
                "Node": { "type": "object", "properties": { "next": { "$ref": "#/components/schemas/Node" } } }
            } }
        });
        let out = inline(
            &root,
            &serde_json::json!({ "$ref": "#/components/schemas/Node" }),
        );
        assert_eq!(out["properties"]["next"], serde_json::json!({}));
    }
}
