// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! A small, format-preserving YAML layer for the subset of YAML that OpenCollection files use.
//!
//! The parser builds a concrete syntax tree: every byte of the input belongs to exactly one
//! piece of the tree (a scalar's source text, or the "glue" between nodes: indentation,
//! indicators, blank lines and comments). Writing concatenates those pieces, so an unchanged
//! tree reproduces its input byte for byte. A scalar whose value is changed through
//! [`Scalar::set_str`] is re-rendered the way Bruno's writer (the `yaml` npm package with
//! Bruno's options) would render it; everything around it stays untouched.
//!
//! Supported: block mappings and sequences, flow mappings and sequences, plain, single- and
//! double-quoted scalars (also multi-line), literal and folded block scalars with chomping
//! and indentation indicators, comments, a leading `---`, LF and CRLF line breaks.
//! Rejected with an error: anchors, aliases, tags, explicit (`?`) and complex keys,
//! directives, multiple documents, duplicate keys and tab indentation.

// Renders changed scalars; reached only through the edit API, which only tests use so far.
#[cfg_attr(not(test), allow(dead_code))]
mod emit;
mod parse;

pub(crate) use parse::parse;

/// One YAML document.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Document {
    /// Everything before the root node (comments, blank lines, a `---` marker).
    pub head: String,
    pub root: Node,
    /// Everything after the root node.
    pub tail: String,
    /// Line break used for newly rendered multi-line text (`"\n"` or `"\r\n"`).
    pub newline: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Scalar(Scalar),
    Map(Map),
    Seq(Seq),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

/// How a plain scalar resolves under the YAML 1.2 core schema. Quoted and block scalars are
/// always strings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Resolved {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scalar {
    /// The decoded value (escapes resolved, lines folded, chomping applied).
    pub value: String,
    pub style: Style,
    /// The exact source text of the scalar, including quotes or the block header.
    pub raw: String,
    /// Indentation of the node that owns this scalar (the column of its key or `-`; `-1` at
    /// the document root). Block scalar content is indented relative to it.
    pub owner_indent: isize,
    pub in_flow: bool,
    pub is_key: bool,
    /// A comment follows on the same line, so the scalar must stay on one line.
    pub comment_after: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Map {
    pub flow: bool,
    /// Column of the keys (block maps).
    pub indent: usize,
    /// `{` for flow maps, empty for block maps.
    pub open: String,
    pub entries: Vec<Entry>,
    /// Trivia and `}` for flow maps, empty for block maps.
    pub close: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    /// Source text between the previous piece and the key.
    pub lead: String,
    pub key: Scalar,
    /// Source text between the key and the value (`: `). When the value is a block
    /// collection, the line break and indentation before it belong to its first entry.
    pub sep: String,
    pub value: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seq {
    pub flow: bool,
    /// Column of the `-` indicators (block sequences).
    pub indent: usize,
    pub open: String,
    pub items: Vec<Item>,
    pub close: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    pub lead: String,
    /// `- ` in block sequences (like [`Entry::sep`]), empty in flow sequences.
    pub dash: String,
    pub value: Node,
}

impl Document {
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(self.head.len() + self.tail.len() + 256);
        out.push_str(&self.head);
        self.root.write(&mut out);
        out.push_str(&self.tail);
        out
    }
}

impl Node {
    fn write(&self, out: &mut String) {
        match self {
            Node::Scalar(s) => out.push_str(&s.raw),
            Node::Map(m) => {
                out.push_str(&m.open);
                for e in &m.entries {
                    out.push_str(&e.lead);
                    out.push_str(&e.key.raw);
                    out.push_str(&e.sep);
                    e.value.write(out);
                }
                out.push_str(&m.close);
            }
            Node::Seq(s) => {
                out.push_str(&s.open);
                for i in &s.items {
                    out.push_str(&i.lead);
                    out.push_str(&i.dash);
                    i.value.write(out);
                }
                out.push_str(&s.close);
            }
        }
    }

    /// Value of `key` if this is a mapping that has it.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(m) => m
                .entries
                .iter()
                .find(|e| e.key.value == key)
                .map(|e| &e.value),
            _ => None,
        }
    }

    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "edit API; only tests edit documents so far")
    )]
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        match self {
            Node::Map(m) => m
                .entries
                .iter_mut()
                .find(|e| e.key.value == key)
                .map(|e| &mut e.value),
            _ => None,
        }
    }

    pub fn as_scalar(&self) -> Option<&Scalar> {
        match self {
            Node::Scalar(s) => Some(s),
            _ => None,
        }
    }

    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "edit API; only tests edit documents so far")
    )]
    pub fn as_scalar_mut(&mut self) -> Option<&mut Scalar> {
        match self {
            Node::Scalar(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_map(&self) -> bool {
        matches!(self, Node::Map(_))
    }

    /// Items of a sequence; empty for anything else.
    pub fn items(&self) -> impl Iterator<Item = &Node> {
        let items: &[Item] = match self {
            Node::Seq(s) => &s.items,
            _ => &[],
        };
        items.iter().map(|i| &i.value)
    }

    pub fn is_seq(&self) -> bool {
        matches!(self, Node::Seq(_))
    }

    /// Text of a non-null scalar (strings, numbers and booleans alike, as written).
    pub fn text(&self) -> Option<&str> {
        match self {
            Node::Scalar(s) if s.resolve() != Resolved::Null => Some(&s.value),
            _ => None,
        }
    }

    /// The value of a scalar that resolves to a string.
    #[cfg(test)]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Scalar(s) if s.resolve() == Resolved::Str => Some(&s.value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.as_scalar()?.resolve() {
            Resolved::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self.as_scalar()?.resolve() {
            Resolved::Int(i) => Some(i as f64),
            Resolved::Float(f) => Some(f),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Node::Scalar(s) if s.resolve() == Resolved::Null)
    }
}

impl Scalar {
    /// Resolves the scalar's type under the YAML 1.2 core schema.
    pub fn resolve(&self) -> Resolved {
        if self.style != Style::Plain {
            return Resolved::Str;
        }
        resolve_plain(&self.value)
    }

    /// Sets a string value. An unchanged value keeps its source text; a changed one is
    /// rendered as Bruno would write it. `newline` is the document's line break.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "edit API; only tests edit documents so far")
    )]
    pub fn set_str(&mut self, value: &str, newline: &str) {
        if self.resolve() == Resolved::Str && self.value == value {
            return;
        }
        let (raw, style) = emit::render_str(
            value,
            &emit::Context {
                owner_indent: self.owner_indent,
                in_flow: self.in_flow,
                implicit_key: self.is_key,
                single_line: self.comment_after,
            },
        );
        self.raw = if newline == "\n" {
            raw
        } else {
            raw.replace('\n', newline)
        };
        self.style = style;
        self.value = value.to_owned();
    }
}

/// YAML 1.2 core schema resolution of a plain scalar (the schema Bruno's `yaml` package uses).
pub(crate) fn resolve_plain(s: &str) -> Resolved {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Resolved::Null,
        "true" | "True" | "TRUE" => return Resolved::Bool(true),
        "false" | "False" | "FALSE" => return Resolved::Bool(false),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => {
            return Resolved::Float(f64::INFINITY);
        }
        "-.inf" | "-.Inf" | "-.INF" => return Resolved::Float(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => return Resolved::Float(f64::NAN),
        _ => {}
    }
    let b = s.as_bytes();
    let digits = |x: &[u8], f: fn(&u8) -> bool| !x.is_empty() && x.iter().all(f);
    if let Some(oct) = s.strip_prefix("0o")
        && digits(oct.as_bytes(), |c| (b'0'..=b'7').contains(c))
    {
        return i64::from_str_radix(oct, 8).map_or(Resolved::Str, Resolved::Int);
    }
    if let Some(hex) = s.strip_prefix("0x")
        && digits(hex.as_bytes(), u8::is_ascii_hexdigit)
    {
        return i64::from_str_radix(hex, 16).map_or(Resolved::Str, Resolved::Int);
    }
    let unsigned = match b.first() {
        Some(b'-' | b'+') => &b[1..],
        _ => b,
    };
    if digits(unsigned, u8::is_ascii_digit) {
        return match s.parse::<i64>() {
            Ok(i) => Resolved::Int(i),
            Err(_) => s.parse::<f64>().map_or(Resolved::Str, Resolved::Float),
        };
    }
    if is_float(unsigned) {
        return s.parse::<f64>().map_or(Resolved::Str, Resolved::Float);
    }
    Resolved::Str
}

/// `(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?` with at least a dot or an exponent.
fn is_float(b: &[u8]) -> bool {
    let mut i = 0;
    let int_digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    let mut dot = false;
    if b.get(i) == Some(&b'.') {
        dot = true;
        i += 1;
        frac_digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    let mut exp = false;
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'-' | b'+')) {
            i += 1;
        }
        let n = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return false;
        }
        i += n;
        exp = true;
    }
    i == b.len() && (dot || exp)
}

#[cfg(test)]
mod tests;
