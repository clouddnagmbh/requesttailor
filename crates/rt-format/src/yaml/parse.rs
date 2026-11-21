// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Parser: source text → concrete syntax tree (see the module docs in `mod.rs`).
//!
//! Every function returns the position where the piece it parsed ends; the caller slices the
//! source between pieces into the tree's glue strings, so no byte gets lost.

use std::cell::Cell;
use std::fmt;

use super::{Document, Entry, Item, Map, Node, Scalar, Seq, Style};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Error {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Parses one YAML document.
pub(crate) fn parse(src: &str) -> Result<Document> {
    let p = Parser {
        src,
        b: src.as_bytes(),
        depth: Cell::new(0),
    };
    p.document()
}

/// Maximum nesting of collections; keeps the recursive parser's stack use bounded.
const MAX_DEPTH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Eol,
    Colon(usize),
    Comment,
    FlowIndicator,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Map,
    Seq,
}

/// A mapping key found at some position.
struct Key {
    scalar: Scalar,
    end: usize,
    colon: usize,
}

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    depth: Cell<usize>,
}

/// Counts one level of collection nesting while alive.
struct Nesting<'p>(&'p Cell<usize>);

impl Drop for Nesting<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

fn is_break(c: u8) -> bool {
    c == b'\n' || c == b'\r'
}

fn is_white(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

fn is_flow_indicator(c: u8) -> bool {
    matches!(c, b',' | b'[' | b']' | b'{' | b'}')
}

impl<'a> Parser<'a> {
    fn len(&self) -> usize {
        self.b.len()
    }

    fn at(&self, p: usize) -> Option<u8> {
        self.b.get(p).copied()
    }

    /// True at the end of input, a line break or white space.
    fn blank_at(&self, p: usize) -> bool {
        self.at(p).is_none_or(|c| is_break(c) || is_white(c))
    }

    fn err<T>(&self, p: usize, message: impl Into<String>) -> Result<T> {
        let p = p.min(self.len());
        let line = self.b[..p].iter().filter(|&&c| c == b'\n').count() + 1;
        Err(Error {
            line,
            column: p - self.line_start(p) + 1,
            message: message.into(),
        })
    }

    /// Enters one level of collection nesting.
    fn nest(&self, p: usize) -> Result<Nesting<'_>> {
        if self.depth.get() >= MAX_DEPTH {
            return self.err(p, format!("nesting deeper than {MAX_DEPTH} levels"));
        }
        self.depth.set(self.depth.get() + 1);
        Ok(Nesting(&self.depth))
    }

    fn line_start(&self, p: usize) -> usize {
        self.b[..p]
            .iter()
            .rposition(|&c| is_break(c))
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self, p: usize) -> usize {
        self.b[p..]
            .iter()
            .position(|&c| is_break(c))
            .map_or(self.len(), |i| p + i)
    }

    /// Position after the line break at `p`.
    fn after_break(&self, p: usize) -> usize {
        if self.at(p) == Some(b'\r') && self.at(p + 1) == Some(b'\n') {
            p + 2
        } else {
            p + 1
        }
    }

    fn col(&self, p: usize) -> usize {
        p - self.line_start(p)
    }

    fn slice(&self, a: usize, b: usize) -> String {
        self.src[a..b].to_owned()
    }

    /// Skips white space, comments and line breaks.
    fn skip_trivia(&self, mut p: usize) -> usize {
        loop {
            while self.at(p).is_some_and(is_white) {
                p += 1;
            }
            match self.at(p) {
                Some(b'#') if p == 0 || self.blank_at(p - 1) => p = self.line_end(p),
                Some(c) if is_break(c) => p = self.after_break(p),
                _ => return p,
            }
        }
    }

    /// Tabs must not be used for indentation.
    fn check_indent(&self, p: usize) -> Result<()> {
        let start = self.line_start(p);
        if self.b[start..p].contains(&b'\t') {
            return self.err(p, "tabs are not allowed for indentation");
        }
        Ok(())
    }

    fn empty_scalar(&self, owner_indent: isize, in_flow: bool) -> Node {
        Node::Scalar(Scalar {
            value: String::new(),
            style: Style::Plain,
            raw: String::new(),
            owner_indent,
            in_flow,
            is_key: false,
            comment_after: false,
        })
    }

    fn document(&self) -> Result<Document> {
        let mut p = self.skip_trivia(0);
        if self.at(p) == Some(b'%') && self.col(p) == 0 {
            return self.err(p, "YAML directives are not supported");
        }
        if self.marker_at(p, b"---") {
            p = self.skip_trivia(p + 3);
        }
        let head = self.slice(0, p);
        let newline = if self.src.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        if p == self.len() {
            return Ok(Document {
                head,
                root: self.empty_scalar(-1, false),
                tail: String::new(),
                newline,
            });
        }
        let (root, end) = match self.collection_at(p)? {
            Some(Kind::Map) => self.block_map(p, self.col(p), String::new())?,
            Some(Kind::Seq) => self.block_seq(p, self.col(p), String::new())?,
            None => self.inline_value(p, -1, -1)?,
        };
        let mut q = self.skip_trivia(end);
        if self.marker_at(q, b"...") {
            q = self.skip_trivia(q + 3);
        }
        if q < self.len() {
            if self.marker_at(q, b"---") {
                return self.err(q, "multiple documents in one file are not supported");
            }
            return self.err(q, "unexpected content");
        }
        Ok(Document {
            head,
            root,
            tail: self.slice(end, self.len()),
            newline,
        })
    }

    /// A document marker (`---` or `...`) at the start of a line.
    fn marker_at(&self, p: usize, marker: &[u8]) -> bool {
        self.col(p) == 0 && self.b[p..].starts_with(marker) && self.blank_at(p + marker.len())
    }

    fn dash_at(&self, p: usize) -> bool {
        self.at(p) == Some(b'-') && self.blank_at(p + 1)
    }

    fn collection_at(&self, p: usize) -> Result<Option<Kind>> {
        if self.dash_at(p) {
            return Ok(Some(Kind::Seq));
        }
        Ok(self.key_at(p)?.map(|_| Kind::Map))
    }

    fn document_end_at(&self, p: usize) -> bool {
        self.marker_at(p, b"---") || self.marker_at(p, b"...")
    }

    /// A single-line implicit key followed by `:` and white space, if one starts at `p`.
    fn key_at(&self, p: usize) -> Result<Option<Key>> {
        let col = self.col(p) as isize;
        let (scalar, end) = match self.at(p) {
            Some(b'"' | b'\'') => {
                let (s, end) = self.quoted(p, col, false)?;
                if self.line_end(p) < end {
                    return Ok(None);
                }
                (s, end)
            }
            Some(_) if self.plain_start(p, false) => {
                let (end, stop) = self.scan_plain_line(p, false);
                let Stop::Colon(colon) = stop else {
                    return Ok(None);
                };
                let s = self.plain_scalar(p, end, col, false, vec![self.slice(p, end)]);
                return Ok(Some(Key {
                    scalar: Scalar { is_key: true, ..s },
                    end,
                    colon,
                }));
            }
            _ => return Ok(None),
        };
        let mut q = end;
        while self.at(q).is_some_and(is_white) {
            q += 1;
        }
        if self.at(q) == Some(b':') && self.blank_at(q + 1) {
            return Ok(Some(Key {
                scalar: Scalar {
                    is_key: true,
                    ..scalar
                },
                end,
                colon: q,
            }));
        }
        Ok(None)
    }

    fn block_map(&self, mut p: usize, indent: usize, mut lead: String) -> Result<(Node, usize)> {
        let _nesting = self.nest(p)?;
        let mut entries: Vec<Entry> = Vec::new();
        let end = loop {
            self.check_indent(p)?;
            let Some(key) = self.key_at(p)? else {
                return self.err(p, "expected a mapping key");
            };
            if entries.iter().any(|e| e.key.value == key.scalar.value) {
                return self.err(p, format!("duplicate key `{}`", key.scalar.value));
            }
            let after = key.colon + 1;
            let (sep, value, vend) = self.value_after(key.end, after, indent, true)?;
            entries.push(Entry {
                lead,
                key: key.scalar,
                sep,
                value,
            });
            let r = self.skip_trivia(vend);
            if r >= self.len() || self.col(r) < indent || self.document_end_at(r) {
                break vend;
            }
            if self.col(r) > indent {
                return self.err(r, "unexpected indentation");
            }
            lead = self.slice(vend, r);
            p = r;
        };
        Ok((
            Node::Map(Map {
                flow: false,
                indent,
                open: String::new(),
                entries,
                close: String::new(),
            }),
            end,
        ))
    }

    fn block_seq(&self, mut p: usize, indent: usize, mut lead: String) -> Result<(Node, usize)> {
        let _nesting = self.nest(p)?;
        let mut items = Vec::new();
        let end = loop {
            self.check_indent(p)?;
            if !self.dash_at(p) {
                return self.err(p, "expected a sequence entry");
            }
            let (dash, value, vend) = self.value_after(p, p + 1, indent, false)?;
            items.push(Item { lead, dash, value });
            let r = self.skip_trivia(vend);
            if r >= self.len() || self.col(r) < indent || self.document_end_at(r) {
                break vend;
            }
            if self.col(r) > indent {
                return self.err(r, "unexpected indentation");
            }
            if !self.dash_at(r) {
                // A mapping key at the same column ends a sequence that is a mapping value.
                break vend;
            }
            lead = self.slice(vend, r);
            p = r;
        };
        Ok((
            Node::Seq(Seq {
                flow: false,
                indent,
                open: String::new(),
                items,
                close: String::new(),
            }),
            end,
        ))
    }

    /// Parses the value after a `:` (mapping) or `-` (sequence) indicator that ends at
    /// `after`. `start` is where the separator glue starts. Returns the separator, the value
    /// and where the value ends.
    fn value_after(
        &self,
        start: usize,
        after: usize,
        indent: usize,
        in_map: bool,
    ) -> Result<(String, Node, usize)> {
        let owner = indent as isize;
        let mut q = after;
        while self.at(q).is_some_and(is_white) {
            q += 1;
        }
        let same_line = self.at(q).is_some_and(|c| !is_break(c) && c != b'#');
        if same_line {
            if !in_map || !self.dash_at(q) {
                if let Some(kind) = self.collection_at(q)? {
                    if in_map {
                        return self.err(q, "mapping values are not allowed here");
                    }
                    // Compact collection in a sequence entry: `- key: value` or `- - x`.
                    let lead = self.slice(after, q);
                    let (node, end) = self.block_collection(kind, q, lead)?;
                    return Ok((self.slice(start, after), node, end));
                }
                let (node, end) = self.inline_value(q, owner, owner)?;
                return Ok((self.slice(start, q), node, end));
            }
            return self.err(q, "sequence entries are not allowed here");
        }
        let r = self.skip_trivia(after);
        if r < self.len() {
            let col = self.col(r);
            if col > indent {
                if let Some(kind) = self.collection_at(r)? {
                    let lead = self.slice(after, r);
                    let (node, end) = self.block_collection(kind, r, lead)?;
                    return Ok((self.slice(start, after), node, end));
                }
                let (node, end) = self.inline_value(r, owner, owner)?;
                return Ok((self.slice(start, r), node, end));
            }
            if in_map && col == indent && self.dash_at(r) {
                // `key:` followed by a sequence at the key's own column.
                let lead = self.slice(after, r);
                let (node, end) = self.block_seq(r, col, lead)?;
                return Ok((self.slice(start, after), node, end));
            }
        }
        Ok((
            self.slice(start, after),
            self.empty_scalar(owner, false),
            after,
        ))
    }

    fn block_collection(&self, kind: Kind, p: usize, lead: String) -> Result<(Node, usize)> {
        let col = self.col(p);
        match kind {
            Kind::Map => self.block_map(p, col, lead),
            Kind::Seq => self.block_seq(p, col, lead),
        }
    }

    /// A scalar or flow collection in block context. `parent` is the indentation that
    /// continuation lines of a multi-line plain scalar must exceed.
    fn inline_value(&self, p: usize, owner: isize, parent: isize) -> Result<(Node, usize)> {
        let (node, end) = match self.at(p) {
            Some(b'|' | b'>') => {
                let (s, end) = self.block_scalar(p, owner)?;
                return Ok((Node::Scalar(s), end));
            }
            Some(b'"' | b'\'') => {
                let (s, end) = self.quoted(p, owner, false)?;
                (Node::Scalar(s), end)
            }
            Some(b'[' | b'{') => self.flow(p, owner)?,
            _ if self.plain_start(p, false) => {
                let (s, end) = self.plain_block(p, owner, parent)?;
                (Node::Scalar(s), end)
            }
            _ => return self.unexpected(p),
        };
        let mut q = end;
        while self.at(q).is_some_and(is_white) {
            q += 1;
        }
        if self.at(q) == Some(b':') && self.blank_at(q + 1) {
            return self.err(q, "complex mapping keys are not supported");
        }
        if q > end && self.at(q) == Some(b'#') {
            let mut node = node;
            if let Node::Scalar(s) = &mut node {
                s.comment_after = true;
            }
            return Ok((node, end));
        }
        if self.at(q).is_some_and(|c| !is_break(c)) {
            return self.err(q, "unexpected content after value");
        }
        Ok((node, end))
    }

    fn unexpected<T>(&self, p: usize) -> Result<T> {
        match self.at(p) {
            Some(b'&') => self.err(p, "anchors are not supported"),
            Some(b'*') => self.err(p, "aliases are not supported"),
            Some(b'!') => self.err(p, "tags are not supported"),
            Some(b'?') => self.err(p, "explicit keys are not supported"),
            Some(b'-') => self.err(p, "sequence entries are not allowed here"),
            Some(c) => self.err(p, format!("unexpected character `{}`", c as char)),
            None => self.err(p, "unexpected end of input"),
        }
    }

    /// Whether a plain scalar may start at `p`.
    fn plain_start(&self, p: usize, flow: bool) -> bool {
        let Some(c) = self.at(p) else { return false };
        match c {
            b'-' | b'?' | b':' => self
                .at(p + 1)
                .is_some_and(|n| !(is_break(n) || is_white(n) || flow && is_flow_indicator(n))),
            b',' | b'[' | b']' | b'{' | b'}' | b'#' | b'&' | b'*' | b'!' | b'|' | b'>' | b'\''
            | b'"' | b'%' | b'@' | b'`' => false,
            c => !is_break(c) && !is_white(c),
        }
    }

    /// Scans plain scalar text on one line. Returns the end of the text (trailing white
    /// space excluded) and why scanning stopped.
    fn scan_plain_line(&self, p: usize, flow: bool) -> (usize, Stop) {
        let mut i = p;
        let mut end = p;
        let stop = loop {
            let Some(c) = self.at(i) else { break Stop::Eol };
            if is_break(c) {
                break Stop::Eol;
            }
            if c == b':' {
                let next = self.at(i + 1);
                if next.is_none_or(|n| is_break(n) || is_white(n))
                    || (flow && next.is_some_and(is_flow_indicator))
                {
                    break Stop::Colon(i);
                }
            }
            if c == b'#' && i > p && self.blank_at(i - 1) {
                break Stop::Comment;
            }
            if flow && is_flow_indicator(c) {
                break Stop::FlowIndicator;
            }
            i += 1;
            if !is_white(c) {
                end = i;
            }
        };
        (end, stop)
    }

    fn plain_scalar(
        &self,
        start: usize,
        end: usize,
        owner: isize,
        in_flow: bool,
        lines: Vec<String>,
    ) -> Scalar {
        Scalar {
            value: fold_lines(&lines),
            style: Style::Plain,
            raw: self.slice(start, end),
            owner_indent: owner,
            in_flow,
            is_key: false,
            comment_after: false,
        }
    }

    /// A plain scalar in block context, possibly continued on more-indented lines.
    fn plain_block(&self, p: usize, owner: isize, parent: isize) -> Result<(Scalar, usize)> {
        let (mut end, mut stop) = self.scan_plain_line(p, false);
        // Lines of the scalar; `""` marks an empty line between text lines.
        let mut lines = vec![self.slice(p, end)];
        while stop == Stop::Eol && end < self.len() {
            let mut q = self.line_end(end);
            let mut empties = 0;
            let text_start = loop {
                if q >= self.len() {
                    break None;
                }
                q = self.after_break(q);
                let mut t = q;
                while self.at(t).is_some_and(is_white) {
                    t += 1;
                }
                match self.at(t) {
                    Some(c) if is_break(c) => {
                        empties += 1;
                        q = t;
                    }
                    None => break None,
                    Some(_) => break Some(t),
                }
            };
            let Some(t) = text_start else { break };
            let indent = self.b[self.line_start(t)..t]
                .iter()
                .take_while(|&&c| c == b' ')
                .count() as isize;
            if indent <= parent
                || self.at(t) == Some(b'#')
                || self.marker_at(t, b"---")
                || self.marker_at(t, b"...")
            {
                break;
            }
            let (e, s) = self.scan_plain_line(t, false);
            if let Stop::Colon(c) = s {
                return self.err(
                    c,
                    "mapping values are not allowed in a multi-line plain scalar",
                );
            }
            lines.extend(std::iter::repeat_n(String::new(), empties));
            lines.push(self.slice(t, e));
            end = e;
            stop = s;
        }
        if let Stop::Colon(c) = stop {
            return self.err(c, "mapping values are not allowed here");
        }
        Ok((self.plain_scalar(p, end, owner, false, lines), end))
    }

    /// A single- or double-quoted scalar starting at `p`.
    fn quoted(&self, p: usize, owner: isize, in_flow: bool) -> Result<(Scalar, usize)> {
        let double = self.at(p) == Some(b'"');
        let mut out = String::new();
        // Length of `out` without trailing literal white space (which folding removes).
        let mut keep = 0;
        let mut i = p + 1;
        let end = loop {
            let Some(c) = self.at(i) else {
                return self.err(p, "unterminated quoted scalar");
            };
            match c {
                b'\'' if !double => {
                    if self.at(i + 1) == Some(b'\'') {
                        out.push('\'');
                        keep = out.len();
                        i += 2;
                    } else {
                        break i + 1;
                    }
                }
                b'"' if double => break i + 1,
                b'\\' if double => {
                    let Some(e) = self.at(i + 1) else {
                        return self.err(i, "unterminated escape sequence");
                    };
                    if is_break(e) {
                        // Escaped line break: joins the lines without a space.
                        i = self.after_break(i + 1);
                        loop {
                            while self.at(i).is_some_and(is_white) {
                                i += 1;
                            }
                            match self.at(i) {
                                Some(c) if is_break(c) => {
                                    out.push('\n');
                                    i = self.after_break(i);
                                }
                                _ => break,
                            }
                        }
                        keep = out.len();
                        continue;
                    }
                    let (ch, len) = self.escape(i)?;
                    out.push(ch);
                    keep = out.len();
                    i += len;
                }
                c if is_break(c) => {
                    out.truncate(keep);
                    i = self.after_break(i);
                    let mut empties = 0;
                    loop {
                        while self.at(i).is_some_and(is_white) {
                            i += 1;
                        }
                        match self.at(i) {
                            Some(c) if is_break(c) => {
                                empties += 1;
                                i = self.after_break(i);
                            }
                            _ => break,
                        }
                    }
                    if self.marker_at(i, b"---") || self.marker_at(i, b"...") {
                        return self.err(i, "document marker inside a quoted scalar");
                    }
                    if empties == 0 {
                        out.push(' ');
                    } else {
                        out.extend(std::iter::repeat_n('\n', empties));
                    }
                    keep = out.len();
                }
                _ => {
                    let ch = self.src[i..].chars().next().expect("char boundary");
                    out.push(ch);
                    if !is_white(c) {
                        keep = out.len();
                    }
                    i += ch.len_utf8();
                }
            }
        };
        Ok((
            Scalar {
                value: out,
                style: if double {
                    Style::DoubleQuoted
                } else {
                    Style::SingleQuoted
                },
                raw: self.slice(p, end),
                owner_indent: owner,
                in_flow,
                is_key: false,
                comment_after: false,
            },
            end,
        ))
    }

    /// Decodes the escape sequence at `p` (a backslash). Returns the character and the
    /// length of the sequence.
    fn escape(&self, p: usize) -> Result<(char, usize)> {
        let e = self.at(p + 1).unwrap_or(0);
        let simple = match e {
            b'0' => Some('\0'),
            b'a' => Some('\x07'),
            b'b' => Some('\x08'),
            b't' | b'\t' => Some('\t'),
            b'n' => Some('\n'),
            b'v' => Some('\x0b'),
            b'f' => Some('\x0c'),
            b'r' => Some('\r'),
            b'e' => Some('\x1b'),
            b' ' => Some(' '),
            b'"' => Some('"'),
            b'/' => Some('/'),
            b'\\' => Some('\\'),
            b'N' => Some('\u{85}'),
            b'_' => Some('\u{a0}'),
            b'L' => Some('\u{2028}'),
            b'P' => Some('\u{2029}'),
            _ => None,
        };
        if let Some(ch) = simple {
            return Ok((ch, 2));
        }
        let digits = match e {
            b'x' => 2,
            b'u' => 4,
            b'U' => 8,
            _ => return self.err(p, "invalid escape sequence"),
        };
        let hex = self
            .src
            .get(p + 2..p + 2 + digits)
            .filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()));
        let ch = hex
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .and_then(char::from_u32);
        match ch {
            Some(ch) => Ok((ch, 2 + digits)),
            None => self.err(p, "invalid escape sequence"),
        }
    }

    /// A literal (`|`) or folded (`>`) block scalar whose indicator is at `p`.
    fn block_scalar(&self, p: usize, owner: isize) -> Result<(Scalar, usize)> {
        #[derive(PartialEq)]
        enum Chomp {
            Strip,
            Clip,
            Keep,
        }
        let literal = self.at(p) == Some(b'|');
        let mut chomp = Chomp::Clip;
        let mut explicit = None;
        let mut i = p + 1;
        for _ in 0..2 {
            match self.at(i) {
                Some(b'-') if chomp == Chomp::Clip => chomp = Chomp::Strip,
                Some(b'+') if chomp == Chomp::Clip => chomp = Chomp::Keep,
                Some(c @ b'1'..=b'9') if explicit.is_none() => explicit = Some((c - b'0') as isize),
                _ => break,
            }
            i += 1;
        }
        let mut h = i;
        while self.at(h).is_some_and(is_white) {
            h += 1;
        }
        if self.at(h) == Some(b'#') && h > i {
            h = self.line_end(h);
        }
        if self.at(h).is_some_and(|c| !is_break(c)) {
            return self.err(h, "invalid block scalar header");
        }
        let header_end = h;

        // (start, end) of each line in the scalar, and whether it has content.
        let mut lines: Vec<(usize, usize, bool)> = Vec::new();
        let mut indent: Option<usize> = explicit.map(|d| (owner + d).max(0) as usize);
        let mut q = header_end;
        while q < self.len() {
            let ls = self.after_break(q);
            if ls >= self.len() {
                break;
            }
            let le = self.line_end(ls);
            let spaces = self.b[ls..le].iter().take_while(|&&c| c == b' ').count();
            let blank = spaces == le - ls;
            let ci = match indent {
                Some(ci) => ci,
                None if blank => {
                    lines.push((ls, le, false));
                    q = le;
                    continue;
                }
                None => {
                    if spaces as isize <= owner {
                        break;
                    }
                    indent = Some(spaces);
                    spaces
                }
            };
            if blank {
                lines.push((ls, le, spaces > ci));
            } else if spaces < ci {
                break;
            } else {
                lines.push((ls, le, true));
            }
            q = le;
        }
        let ci = indent.unwrap_or(0);
        let last_content = lines.iter().rposition(|l| l.2);
        let raw_end = match (&chomp, last_content) {
            (Chomp::Keep, _) => lines.last().map_or(header_end, |l| l.1),
            (_, Some(i)) => lines[i].1,
            (_, None) => header_end,
        };
        let has_break_after = |e: usize| e < self.len();

        let mut value = String::new();
        if let Some(last) = last_content {
            let text = |l: &(usize, usize, bool)| -> &str {
                if l.2 { &self.src[l.0 + ci..l.1] } else { "" }
            };
            if literal {
                for (n, l) in lines[..=last].iter().enumerate() {
                    if n > 0 {
                        value.push('\n');
                    }
                    value.push_str(text(l));
                }
            } else {
                fold_block(&lines[..=last], text, &mut value);
            }
            let after = &lines[last + 1..];
            let breaks = after.len()
                + usize::from(has_break_after(after.last().map_or(lines[last].1, |l| l.1)));
            match chomp {
                Chomp::Strip => {}
                Chomp::Clip if breaks > 0 => value.push('\n'),
                Chomp::Clip => {}
                Chomp::Keep => value.extend(std::iter::repeat_n('\n', breaks)),
            }
        } else if chomp == Chomp::Keep {
            let breaks = lines.iter().filter(|l| has_break_after(l.1)).count();
            value.extend(std::iter::repeat_n('\n', breaks));
        }
        Ok((
            Scalar {
                value,
                style: if literal {
                    Style::Literal
                } else {
                    Style::Folded
                },
                raw: self.slice(p, raw_end),
                owner_indent: owner,
                in_flow: false,
                is_key: false,
                comment_after: false,
            },
            raw_end,
        ))
    }

    /// Skips white space, line breaks and comments inside a flow collection.
    fn skip_flow_trivia(&self, p: usize) -> usize {
        self.skip_trivia(p)
    }

    /// A flow mapping or sequence starting at `p` (`{` or `[`).
    fn flow(&self, p: usize, owner: isize) -> Result<(Node, usize)> {
        let _nesting = self.nest(p)?;
        let is_map = self.at(p) == Some(b'{');
        let close_char = if is_map { b'}' } else { b']' };
        let mut entries: Vec<Entry> = Vec::new();
        let mut items: Vec<Item> = Vec::new();
        let mut i = p + 1;
        let close;
        loop {
            let mut t = self.skip_flow_trivia(i);
            if self.at(t) == Some(close_char) {
                close = self.slice(i, t + 1);
                i = t + 1;
                break;
            }
            if !entries.is_empty() || !items.is_empty() {
                if self.at(t) != Some(b',') {
                    return self.err(t, "expected `,` in flow collection");
                }
                t = self.skip_flow_trivia(t + 1);
                if self.at(t) == Some(close_char) {
                    close = self.slice(i, t + 1);
                    i = t + 1;
                    break;
                }
            }
            if self.at(t).is_none() {
                return self.err(p, "unterminated flow collection");
            }
            let lead = self.slice(i, t);
            if is_map {
                let (key, kend) = match self.at(t) {
                    Some(b'"' | b'\'') => self.quoted(t, owner, true)?,
                    _ if self.plain_start(t, true) => self.plain_flow(t, owner)?,
                    _ => return self.unexpected(t),
                };
                if entries.iter().any(|e| e.key.value == key.value) {
                    return self.err(t, format!("duplicate key `{}`", key.value));
                }
                let key = Scalar {
                    is_key: true,
                    ..key
                };
                let s = self.skip_flow_trivia(kend);
                let (sep, value, vend) = if self.at(s) == Some(b':') {
                    let v = self.skip_flow_trivia(s + 1);
                    if matches!(self.at(v), Some(b',') | Some(b'}')) {
                        (
                            self.slice(kend, s + 1),
                            self.empty_scalar(owner, true),
                            s + 1,
                        )
                    } else {
                        let (node, vend) = self.flow_value(v, owner)?;
                        (self.slice(kend, v), node, vend)
                    }
                } else {
                    (String::new(), self.empty_scalar(owner, true), kend)
                };
                entries.push(Entry {
                    lead,
                    key,
                    sep,
                    value,
                });
                i = vend;
            } else {
                let (value, vend) = self.flow_value(t, owner)?;
                let s = self.skip_flow_trivia(vend);
                if self.at(s) == Some(b':') {
                    return self.err(s, "mappings inside flow sequences are not supported");
                }
                items.push(Item {
                    lead,
                    dash: String::new(),
                    value,
                });
                i = vend;
            }
        }
        let indent = self.col(p);
        let open = self.slice(p, p + 1);
        let node = if is_map {
            Node::Map(Map {
                flow: true,
                indent,
                open,
                entries,
                close,
            })
        } else {
            Node::Seq(Seq {
                flow: true,
                indent,
                open,
                items,
                close,
            })
        };
        Ok((node, i))
    }

    fn flow_value(&self, p: usize, owner: isize) -> Result<(Node, usize)> {
        match self.at(p) {
            Some(b'[' | b'{') => self.flow(p, owner),
            Some(b'"' | b'\'') => {
                let (s, end) = self.quoted(p, owner, true)?;
                Ok((Node::Scalar(s), end))
            }
            _ if self.plain_start(p, true) => {
                let (s, end) = self.plain_flow(p, owner)?;
                Ok((Node::Scalar(s), end))
            }
            _ => self.unexpected(p),
        }
    }

    /// A plain scalar inside a flow collection (single line only).
    fn plain_flow(&self, p: usize, owner: isize) -> Result<(Scalar, usize)> {
        let (end, _) = self.scan_plain_line(p, true);
        Ok((
            self.plain_scalar(p, end, owner, true, vec![self.slice(p, end)]),
            end,
        ))
    }
}

/// Folds the lines of a plain scalar: single line breaks become spaces, each empty line a
/// line feed. Lines are already trimmed; `""` marks an empty line.
fn fold_lines(lines: &[String]) -> String {
    let mut out = String::new();
    let mut empties = 0;
    for (n, line) in lines.iter().enumerate() {
        if line.is_empty() && n > 0 {
            empties += 1;
            continue;
        }
        if n > 0 {
            if empties == 0 {
                out.push(' ');
            } else {
                out.extend(std::iter::repeat_n('\n', empties));
            }
        }
        empties = 0;
        out.push_str(line.trim_start_matches([' ', '\t']));
    }
    out
}

/// Folds the content lines of a folded block scalar (`>`), up to its last content line.
fn fold_block<'s>(
    lines: &[(usize, usize, bool)],
    text: impl Fn(&(usize, usize, bool)) -> &'s str,
    out: &mut String,
) {
    let mut pending = 0;
    let mut prev_more: Option<bool> = None;
    for l in lines {
        let t = text(l);
        if !l.2 {
            pending += 1;
            continue;
        }
        let more = t.starts_with([' ', '\t']);
        match prev_more {
            None => out.extend(std::iter::repeat_n('\n', pending)),
            Some(pm) => {
                let keep_breaks = more || pm;
                if pending == 0 {
                    out.push(if keep_breaks { '\n' } else { ' ' });
                } else {
                    let n = if keep_breaks { pending + 1 } else { pending };
                    out.extend(std::iter::repeat_n('\n', n));
                }
            }
        }
        out.push_str(t);
        pending = 0;
        prev_more = Some(more);
    }
}
