// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A generic document tree for the importers' input: JSON (own parser, keeps key order) and
//! YAML (this crate's YAML module). Keys keep their source order, which matters for OpenAPI
//! `paths` and Insomnia's objects; nesting is bounded so hostile input cannot exhaust the
//! stack.

use crate::yaml::{self, Resolved, Style};

/// Deepest nesting accepted in an input document.
pub(crate) const MAX_DEPTH: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    /// A number as written in the source.
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

const EMPTY: &[Value] = &[];
const EMPTY_OBJ: &[(String, Value)] = &[];

impl Value {
    /// Value of `key` in an object (the last one if a key repeats, as JSON parsers do).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(entries) => entries.iter().rev().find(|(k, _)| k == key).map(|e| &e.1),
            _ => None,
        }
    }

    /// A non-null value of `key`.
    pub fn field(&self, key: &str) -> Option<&Value> {
        self.get(key).filter(|v| !v.is_null())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A string, or a number or boolean as text.
    pub fn text(&self) -> Option<String> {
        match self {
            Value::Str(s) | Value::Num(s) => Some(s.clone()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    /// `text` of `key`.
    pub fn str_of(&self, key: &str) -> Option<String> {
        self.get(key).and_then(Value::text)
    }

    /// `text` of `key`, or empty.
    pub fn string(&self, key: &str) -> String {
        self.str_of(key).unwrap_or_default()
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Str(s) if s == "true" => Some(true),
            Value::Str(s) if s == "false" => Some(false),
            _ => None,
        }
    }

    /// `true` if `key` is the boolean `true` (or the string "true").
    pub fn flag(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool) == Some(true)
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => n.parse().ok(),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// Items of an array; empty for anything else.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Arr(items) => items,
            _ => EMPTY,
        }
    }

    /// Entries of an object; empty for anything else.
    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Obj(entries) => entries,
            _ => EMPTY_OBJ,
        }
    }

    pub fn is_obj(&self) -> bool {
        matches!(self, Value::Obj(_))
    }

    pub fn is_arr(&self) -> bool {
        matches!(self, Value::Arr(_))
    }

    /// Compact JSON text of this value (key order kept).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out, None, 0);
        out
    }

    /// JSON text indented by two spaces.
    pub fn to_json_pretty(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out, Some(2), 0);
        out
    }

    fn write_json(&self, out: &mut String, indent: Option<usize>, level: usize) {
        let newline = |out: &mut String, level: usize| {
            if let Some(n) = indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * level));
            }
        };
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => out.push_str(n),
            Value::Str(s) => json_string(s, out),
            Value::Arr(items) if items.is_empty() => out.push_str("[]"),
            Value::Obj(entries) if entries.is_empty() => out.push_str("{}"),
            Value::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, level + 1);
                    item.write_json(out, indent, level + 1);
                }
                newline(out, level);
                out.push(']');
            }
            Value::Obj(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, level + 1);
                    json_string(k, out);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    v.write_json(out, indent, level + 1);
                }
                newline(out, level);
                out.push('}');
            }
        }
    }
}

fn json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses JSON or, if the text is not JSON, YAML.
pub(crate) fn parse_any(text: &str) -> Result<Value, String> {
    let trimmed = text.trim_start();
    if trimmed.starts_with(['{', '[']) {
        match parse_json(text) {
            Ok(v) => return Ok(v),
            Err(json) => {
                return parse_yaml(text).map_err(|_| json);
            }
        }
    }
    parse_yaml(text)
}

/// Parses YAML with this crate's parser.
pub(crate) fn parse_yaml(text: &str) -> Result<Value, String> {
    let doc = yaml::parse(text).map_err(|e| e.to_string())?;
    Ok(from_yaml(&doc.root))
}

fn from_yaml(node: &yaml::Node) -> Value {
    match node {
        yaml::Node::Scalar(s) => {
            if s.style != Style::Plain {
                return Value::Str(s.value.clone());
            }
            match s.resolve() {
                Resolved::Null => Value::Null,
                Resolved::Bool(b) => Value::Bool(b),
                Resolved::Int(_) | Resolved::Float(_) => Value::Num(s.value.clone()),
                Resolved::Str => Value::Str(s.value.clone()),
            }
        }
        yaml::Node::Map(m) => Value::Obj(
            m.entries
                .iter()
                .map(|e| (e.key.value.clone(), from_yaml(&e.value)))
                .collect(),
        ),
        yaml::Node::Seq(s) => Value::Arr(s.items.iter().map(|i| from_yaml(&i.value)).collect()),
    }
}

/// Parses JSON (RFC 8259). Duplicate keys are kept; [`Value::get`] finds the last one.
pub(crate) fn parse_json(text: &str) -> Result<Value, String> {
    let mut p = Json {
        b: text.as_bytes(),
        src: text,
        pos: 0,
        depth: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.pos != p.b.len() {
        return Err(p.err("unexpected text after the JSON value"));
    }
    Ok(v)
}

struct Json<'a> {
    b: &'a [u8],
    src: &'a str,
    pos: usize,
    depth: usize,
}

impl Json<'_> {
    fn err(&self, message: &str) -> String {
        let before = &self.b[..self.pos.min(self.b.len())];
        let line = before.iter().filter(|c| **c == b'\n').count() + 1;
        let column = before.iter().rev().take_while(|c| **c != b'\n').count() + 1;
        format!("line {line}, column {column}: {message}")
    }

    fn ws(&mut self) {
        while let Some(c) = self.b.get(self.pos) {
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.b.get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.b.get(self.pos) {
            None => Err(self.err("unexpected end of input")),
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
        }
    }

    fn nested(&mut self, f: fn(&mut Self) -> Result<Value, String>) -> Result<Value, String> {
        if self.depth >= MAX_DEPTH {
            return Err(self.err(&format!("nested deeper than {MAX_DEPTH} levels")));
        }
        self.depth += 1;
        let v = f(self);
        self.depth -= 1;
        v
    }

    fn word(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.b[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(self.err("unexpected character"))
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        self.eat(b'-');
        let digits = |p: &mut Self| {
            let s = p.pos;
            while p.b.get(p.pos).is_some_and(u8::is_ascii_digit) {
                p.pos += 1;
            }
            p.pos > s
        };
        if !digits(self) {
            return Err(self.err("invalid number"));
        }
        if self.eat(b'.') && !digits(self) {
            return Err(self.err("invalid number"));
        }
        if self.eat(b'e') || self.eat(b'E') {
            if !self.eat(b'+') {
                self.eat(b'-');
            }
            if !digits(self) {
                return Err(self.err("invalid number"));
            }
        }
        Ok(Value::Num(self.src[start..self.pos].to_owned()))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let hex = self
            .src
            .get(self.pos..self.pos + 4)
            .filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| self.err("invalid \\u escape"))?;
        self.pos += 4;
        Ok(u32::from_str_radix(hex, 16).expect("checked hex digits"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(&c) = self.b.get(self.pos) {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(&self.src[start..self.pos]);
            match self.b.get(self.pos) {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let Some(&e) = self.b.get(self.pos) else {
                        return Err(self.err("unterminated string"));
                    };
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi)
                                && self.b[self.pos..].starts_with(b"\\u")
                            {
                                self.pos += 2;
                                let lo = self.hex4()?;
                                if (0xDC00..0xE000).contains(&lo) {
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                } else {
                                    0xFFFD
                                }
                            } else {
                                hi
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(self.err("invalid escape")),
                    }
                }
                Some(_) => return Err(self.err("control character in string")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.eat(b']') {
            return Ok(Value::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                return Ok(Value::Arr(items));
            }
            return Err(self.err("expected `,` or `]`"));
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.pos += 1;
        let mut entries = Vec::new();
        self.ws();
        if self.eat(b'}') {
            return Ok(Value::Obj(entries));
        }
        loop {
            self.ws();
            if self.b.get(self.pos) != Some(&b'"') {
                return Err(self.err("expected a string key"));
            }
            let key = self.string()?;
            self.ws();
            if !self.eat(b':') {
                return Err(self.err("expected `:`"));
            }
            self.ws();
            let v = self.value()?;
            entries.push((key, v));
            self.ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Ok(Value::Obj(entries));
            }
            return Err(self.err("expected `,` or `}`"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips_and_keeps_order() {
        let v = parse_json(r#"{"b": 1, "a": [true, null, "x\u00e9\ud83d\ude00"], "c": -1.5e3}"#)
            .unwrap();
        assert_eq!(v.entries()[0].0, "b");
        assert_eq!(v.get("a").unwrap().items()[2].as_str(), Some("xé😀"));
        assert_eq!(v.to_json(), r#"{"b":1,"a":[true,null,"xé😀"],"c":-1.5e3}"#);
    }

    #[test]
    fn json_errors_do_not_panic() {
        for bad in [
            "",
            "{",
            "[1,",
            "\"abc",
            "{\"a\" 1}",
            "01x",
            "[1 2]",
            "\"\\u12\"",
            "-",
            "1.",
            "nul",
            "\"\\ud800\"",
            "{\"a\":1}}",
        ] {
            let _ = parse_json(bad);
        }
        assert!(parse_json(&"[".repeat(10_000)).is_err());
        assert!(parse_json(&format!("{}{}", "[".repeat(150), "]".repeat(150))).is_ok());
    }

    #[test]
    fn yaml_values() {
        let v = parse_any("a: 1\nb: 'x'\nc: [1, two]\nd: ~\n").unwrap();
        assert_eq!(v.get("a"), Some(&Value::Num("1".into())));
        assert_eq!(v.get("b").unwrap().as_str(), Some("x"));
        assert_eq!(v.get("c").unwrap().items().len(), 2);
        assert!(v.get("d").unwrap().is_null());
        assert_eq!(
            v.to_json_pretty(),
            "{\n  \"a\": 1,\n  \"b\": \"x\",\n  \"c\": [\n    1,\n    \"two\"\n  ],\n  \"d\": null\n}"
        );
    }
}
