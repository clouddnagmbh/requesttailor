// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A small ordered JSON tree for the minimal-case search. `serde_json::Value` would sort the
//! keys of objects (the `preserve_order` feature is not on), and the request that comes out
//! must read like the one that went in: same key order, leaves byte for byte as written
//! (numbers, escapes), only the parts that were removed are gone.

/// One JSON value; leaves (`string`, number, `true`, `false`, `null`) keep their source text.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Object(Vec<(String, Node)>),
    Array(Vec<Node>),
    /// A string leaf: the raw source text with its quotes.
    Str(String),
    /// A number, `true`, `false` or `null`: the raw source text.
    Other(String),
}

/// Containers nest at most this deep; deeper input is not parsed (the body stays one part).
const MAX_DEPTH: usize = 64;

/// Parses `text`; `None` if it is not one complete JSON value (a body with `{{variables}}`
/// outside strings, for one).
pub fn parse(text: &str) -> Option<Node> {
    let mut p = Parser {
        bytes: text.as_bytes(),
        text,
        at: 0,
    };
    p.space();
    let node = p.value(0)?;
    p.space();
    (p.at == p.bytes.len()).then_some(node)
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<Node> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.bytes.get(self.at)? {
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                self.space();
                if self.eat(b'}') {
                    return Some(Node::Object(members));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    // Keys are kept decoded, so duplicates and escapes compare as the server
                    // sees them.
                    let key: String = serde_json::from_str(&key).ok()?;
                    self.space();
                    if !self.eat(b':') {
                        return None;
                    }
                    self.space();
                    let value = self.value(depth + 1)?;
                    members.push((key, value));
                    self.space();
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b'}') {
                        return Some(Node::Object(members));
                    }
                    return None;
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.eat(b']') {
                    return Some(Node::Array(items));
                }
                loop {
                    self.space();
                    items.push(self.value(depth + 1)?);
                    self.space();
                    if self.eat(b',') {
                        continue;
                    }
                    if self.eat(b']') {
                        return Some(Node::Array(items));
                    }
                    return None;
                }
            }
            b'"' => self.string().map(Node::Str),
            _ => {
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(|b| {
                    !matches!(b, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r')
                }) {
                    self.at += 1;
                }
                let raw = self.text.get(start..self.at)?;
                // Only what JSON allows as a bare value.
                serde_json::from_str::<serde_json::Value>(raw).ok()?;
                Some(Node::Other(raw.to_owned()))
            }
        }
    }

    /// The raw text of a string, quotes included.
    fn string(&mut self) -> Option<String> {
        let start = self.at;
        if !self.eat(b'"') {
            return None;
        }
        loop {
            match self.bytes.get(self.at)? {
                b'"' => {
                    self.at += 1;
                    break;
                }
                b'\\' => self.at += 2,
                _ => self.at += 1,
            }
        }
        let raw = self.text.get(start..self.at)?;
        serde_json::from_str::<String>(raw).ok()?;
        Some(raw.to_owned())
    }
}

/// Writes the tree with two-space indentation (an empty container as `{}` / `[]`).
pub fn write(node: &Node) -> String {
    let mut out = String::new();
    put(node, 0, &mut out);
    out
}

fn put(node: &Node, level: usize, out: &mut String) {
    let pad = |out: &mut String, level: usize| {
        out.push('\n');
        for _ in 0..level {
            out.push_str("  ");
        }
    };
    match node {
        Node::Str(raw) | Node::Other(raw) => out.push_str(raw),
        Node::Object(members) if members.is_empty() => out.push_str("{}"),
        Node::Array(items) if items.is_empty() => out.push_str("[]"),
        Node::Object(members) => {
            out.push('{');
            for (i, (key, value)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, level + 1);
                out.push_str(&serde_json::to_string(key).unwrap_or_default());
                out.push_str(": ");
                put(value, level + 1, out);
            }
            pad(out, level);
            out.push('}');
        }
        Node::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, level + 1);
                put(item, level + 1, out);
            }
            pad(out, level);
            out.push(']');
        }
    }
}
