// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Renders string scalars the way Bruno writes them.
//!
//! Bruno serializes OpenCollection files with the `yaml` npm package (ISC) and the options
//! `lineWidth: 0, indent: 2, minContentWidth: 0, defaultStringType: 'PLAIN'`. The rules
//! below follow that package's `stringifyString` for those options: plain where the result
//! reads back as the same string, a literal block scalar for multi-line text, otherwise
//! quoted (single quotes when the text contains `"` but no `'`, else double quotes).

use super::{Resolved, Style, resolve_plain};

#[derive(Clone, Copy)]
pub(crate) struct Context {
    /// Indentation of the owning key or `-` (`-1` at the document root).
    pub owner_indent: isize,
    pub in_flow: bool,
    pub implicit_key: bool,
    /// Something follows on the same line (a comment), so no block scalar and no folding.
    pub single_line: bool,
}

impl Context {
    /// Indentation of the scalar's continuation lines. A document-level scalar has none,
    /// unless a line of it would read as a document marker.
    fn indent_for(&self, value: &str) -> String {
        if self.owner_indent >= 0 {
            " ".repeat(self.owner_indent as usize + 2)
        } else if contains_document_marker(value) {
            "  ".to_owned()
        } else {
            String::new()
        }
    }
}

/// Returns the source text for `value` and its style. Multi-line output uses `\n`.
pub(crate) fn render_str(value: &str, ctx: &Context) -> (String, Style) {
    // Control characters can only be written escaped.
    if value
        .chars()
        .any(|c| matches!(c, '\0'..='\x08' | '\x0b'..='\x1f' | '\x7f'..='\u{9f}'))
    {
        return (double_quoted(value, ctx), Style::DoubleQuoted);
    }
    if ctx.single_line && value.contains('\n') {
        // Like a key: one double-quoted line with `\n` escapes.
        let one_line = Context {
            implicit_key: true,
            ..*ctx
        };
        return (double_quoted(value, &one_line), Style::DoubleQuoted);
    }
    plain(value, ctx)
}

fn plain(value: &str, ctx: &Context) -> (String, Style) {
    if (ctx.implicit_key && value.contains('\n'))
        || (ctx.in_flow && value.contains([',', '[', ']', '{', '}']))
    {
        return quoted(value, ctx);
    }
    if needs_quotes(value) {
        return if ctx.implicit_key || ctx.in_flow || !value.contains('\n') {
            quoted(value, ctx)
        } else {
            block(value, ctx)
        };
    }
    if !ctx.implicit_key && !ctx.in_flow && value.contains('\n') {
        return block(value, ctx);
    }
    if ctx.implicit_key && contains_document_marker(value) {
        return quoted(value, ctx);
    }
    if value.contains('\n') || resolve_plain(value) != Resolved::Str {
        // Multi-line text in flow context, or text that would read back as another type.
        return quoted(value, ctx);
    }
    (value.to_owned(), Style::Plain)
}

/// The `yaml` package's test for text that cannot be written as a plain scalar:
/// `^[\n\t ,[\]{}#&*!|>'"%@\x60]|^[?-]$|^[?-][ \t]|[\n:][ \t]|[ \t]\n|[\n\t ]#|[\n\t :]$`.
fn needs_quotes(v: &str) -> bool {
    let b = v.as_bytes();
    let Some(&first) = b.first() else {
        return false;
    };
    if b"\n\t ,[]{}#&*!|>'\"%@`".contains(&first) {
        return true;
    }
    if matches!(first, b'?' | b'-') && (b.len() == 1 || matches!(b[1], b' ' | b'\t')) {
        return true;
    }
    for w in b.windows(2) {
        let (x, y) = (w[0], w[1]);
        if (matches!(x, b'\n' | b':') && matches!(y, b' ' | b'\t'))
            || (matches!(x, b' ' | b'\t') && y == b'\n')
            || (matches!(x, b'\n' | b'\t' | b' ') && y == b'#')
        {
            return true;
        }
    }
    matches!(b[b.len() - 1], b'\n' | b'\t' | b' ' | b':')
}

/// `^(%|---|\.\.\.)` on any line.
fn contains_document_marker(v: &str) -> bool {
    v.split('\n')
        .any(|l| l.starts_with('%') || l.starts_with("---") || l.starts_with("..."))
}

fn quoted(value: &str, ctx: &Context) -> (String, Style) {
    let has_double = value.contains('"');
    let has_single = value.contains('\'');
    if has_double && !has_single {
        single_quoted(value, ctx)
    } else {
        (double_quoted(value, ctx), Style::DoubleQuoted)
    }
}

fn single_quoted(value: &str, ctx: &Context) -> (String, Style) {
    let around_newline = value.contains(" \n")
        || value.contains("\t\n")
        || value.contains("\n ")
        || value.contains("\n\t");
    if (ctx.implicit_key && value.contains('\n')) || around_newline {
        return (double_quoted(value, ctx), Style::DoubleQuoted);
    }
    let indent = ctx.indent_for(value);
    let mut out = String::from("'");
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => out.push_str("''"),
            '\n' => {
                // A run of n line feeds is written as n + 1 line breaks.
                out.push('\n');
                while chars.peek() == Some(&'\n') {
                    chars.next();
                    out.push('\n');
                }
                out.push('\n');
                out.push_str(&indent);
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    (out, Style::SingleQuoted)
}

/// `JSON.stringify` followed by the `yaml` package's rewrite of escapes into YAML ones and of
/// `\n` into folded line breaks (for strings of at least 40 characters, outside keys).
fn double_quoted(value: &str, ctx: &Context) -> String {
    // The escapes `JSON.stringify` produces, already rewritten to their YAML forms.
    let mut json = String::with_capacity(value.len() + 2);
    json.push('"');
    for c in value.chars() {
        match c {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\x08' => json.push_str("\\b"),
            '\x0c' => json.push_str("\\f"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            '\0' => json.push_str("\\0"),
            '\x07' => json.push_str("\\a"),
            '\x0b' => json.push_str("\\v"),
            '\x1b' => json.push_str("\\e"),
            c if (c as u32) < 0x20 => json.push_str(&format!("\\x{:02x}", c as u32)),
            c => json.push(c),
        }
    }
    json.push('"');

    // Length of the original `JSON.stringify` output, which decides about folding.
    let json_len: usize = value
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\x08' | '\x0c' | '\n' | '\r' | '\t' => 2,
            c if (c as u32) < 0x20 => 6,
            // JavaScript counts UTF-16 code units.
            c => c.len_utf16(),
        })
        .sum::<usize>()
        + 2;
    if ctx.implicit_key || json_len < 40 {
        return json;
    }
    let indent = ctx.indent_for(value);
    let b = json.as_bytes();
    let mut out = String::with_capacity(json.len() + 16);
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b' ' && b.get(i + 1) == Some(&b'\\') && b.get(i + 2) == Some(&b'n') {
            // A space before a line break must be escaped so folding keeps it.
            out.push_str(&json[start..i]);
            out.push_str("\\ ");
            i += 1;
            start = i;
        }
        if b[i] == b'\\' {
            if b.get(i + 1) == Some(&b'n') && b.get(i + 2) != Some(&b'"') {
                out.push_str(&json[start..i]);
                out.push_str("\n\n");
                while b.get(i + 2) == Some(&b'\\')
                    && b.get(i + 3) == Some(&b'n')
                    && b.get(i + 4) != Some(&b'"')
                {
                    out.push('\n');
                    i += 2;
                }
                out.push_str(&indent);
                if b.get(i + 2) == Some(&b' ') {
                    out.push('\\');
                }
                i += 2;
                start = i;
                continue;
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    out.push_str(&json[start..]);
    out
}

/// A literal block scalar (Bruno never writes folded ones: `lineWidth: 0` disables folding).
fn block(value: &str, ctx: &Context) -> (String, Style) {
    // A block scalar cannot end in white space unless its last line has content.
    let tail_ws = value.trim_end_matches([' ', '\t']);
    if tail_ws.len() < value.len() && tail_ws.ends_with('\n') {
        return quoted(value, ctx);
    }
    let indent = ctx.indent_for(value);
    let mut body = value;

    let end_start = body.trim_end_matches(['\n', '\t', ' ']).len();
    let mut end = body[end_start..].to_owned();
    let chomp = match end.find('\n') {
        None => "-",
        Some(nl) if end.len() == body.len() || nl != end.len() - 1 => "+",
        Some(_) => "",
    };
    if !end.is_empty() {
        body = &body[..end_start];
        if end.ends_with('\n') {
            end.pop();
        }
        end = indent_inner_newline_runs(&end, &indent);
    }

    // Leading white space: an indentation indicator is needed if it contains spaces, and
    // leading line feeds are written as empty lines.
    let mut start_with_space = false;
    let mut last_nl = None;
    for (i, c) in body.bytes().enumerate() {
        match c {
            b' ' => start_with_space = true,
            b'\n' => last_nl = Some(i),
            _ => break,
        }
    }
    let start_len = last_nl.map_or(0, |n| n + 1);
    let start = indent_newline_runs(&body[..start_len], &indent);
    body = &body[start_len..];

    let indicator = if indent.is_empty() { "1" } else { "2" };
    let header = format!("{}{chomp}", if start_with_space { indicator } else { "" });
    let body = indent_newline_runs(body, &indent);
    (
        format!("|{header}\n{indent}{start}{body}{end}"),
        Style::Literal,
    )
}

/// Appends `indent` after every run of line feeds (`/\n+/g` → `$&indent`).
fn indent_newline_runs(s: &str, indent: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' && chars.peek() != Some(&'\n') {
            out.push_str(indent);
        }
    }
    out
}

/// Like [`indent_newline_runs`], but not for a run at the very end.
fn indent_inner_newline_runs(s: &str, indent: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' && chars.peek().is_some_and(|&n| n != '\n') {
            out.push_str(indent);
        }
    }
    out
}
