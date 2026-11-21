// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Structural edits of a parsed document: setting, inserting and removing mapping entries and
//! sequence items, and rendering new subtrees the way Bruno writes them.
//!
//! New subtrees are rendered to text and stored as a *verbatim* node: a scalar whose source
//! text is the whole rendered block. Writing only concatenates source text, so the output is
//! correct, but the tree below a verbatim node is not navigable. Callers therefore re-parse
//! the document after a batch of edits ([`reparse`]) before reading or editing it again.
//!
//! Layout of rendered text (Bruno: the `yaml` package with `indent: 2`): nested mappings and
//! the `-` of nested sequences are indented by two columns relative to their key; the first
//! entry of a mapping inside a sequence item follows the `- ` on the same line.

use super::emit::{Context, render_str};
use super::{Document, Entry, Item, Map, Node, Scalar, Style, parse};

/// A value to write as a new subtree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Val {
    /// A string, quoted only where YAML needs it.
    Str(String),
    /// A plain scalar written as is: booleans and numbers.
    Plain(String),
    Map(Vec<(&'static str, Val)>),
    Seq(Vec<Val>),
}

impl Val {
    pub fn str(s: impl Into<String>) -> Self {
        Val::Str(s.into())
    }

    pub fn bool(b: bool) -> Self {
        Val::Plain(if b { "true" } else { "false" }.to_owned())
    }
}

fn spaces(n: usize) -> String {
    " ".repeat(n)
}

fn scalar_text(s: &str, owner: usize) -> String {
    render_str(
        s,
        &Context {
            owner_indent: owner as isize,
            in_flow: false,
            implicit_key: false,
            single_line: false,
        },
    )
    .0
}

/// Separator and source text for `v` as the value of a key at column `col`. Multi-line text
/// uses `\n`.
fn entry_value(v: &Val, col: usize) -> (String, String) {
    match v {
        Val::Str(s) => (": ".into(), scalar_text(s, col)),
        Val::Plain(p) => (": ".into(), p.clone()),
        Val::Map(entries) if entries.is_empty() => (": ".into(), "{}".into()),
        Val::Seq(items) if items.is_empty() => (": ".into(), "[]".into()),
        Val::Map(entries) => {
            let mut raw = String::new();
            for (k, v) in entries {
                raw.push('\n');
                raw.push_str(&spaces(col + 2));
                raw.push_str(&entry_text(k, v, col + 2));
            }
            (":".into(), raw)
        }
        Val::Seq(items) => {
            let mut raw = String::new();
            for item in items {
                raw.push('\n');
                raw.push_str(&spaces(col + 2));
                raw.push_str(&item_text(item, col + 2));
            }
            (":".into(), raw)
        }
    }
}

/// `key: value` for a key at column `col`.
fn entry_text(key: &str, v: &Val, col: usize) -> String {
    let (sep, raw) = entry_value(v, col);
    format!("{key}{sep}{raw}")
}

/// A sequence item whose `-` is at column `col`, starting with the `-`.
pub(crate) fn item_text(v: &Val, col: usize) -> String {
    match v {
        Val::Map(entries) if !entries.is_empty() => {
            let mut out = String::from("- ");
            for (n, (k, v)) in entries.iter().enumerate() {
                if n > 0 {
                    out.push('\n');
                    out.push_str(&spaces(col + 2));
                }
                out.push_str(&entry_text(k, v, col + 2));
            }
            out
        }
        Val::Seq(items) if !items.is_empty() => {
            let mut out = String::from("-");
            for item in items {
                out.push('\n');
                out.push_str(&spaces(col + 2));
                out.push_str(&item_text(item, col + 2));
            }
            out
        }
        Val::Map(_) => "- {}".into(),
        Val::Seq(_) => "- []".into(),
        Val::Str(s) => format!("- {}", scalar_text(s, col)),
        Val::Plain(p) => format!("- {p}"),
    }
}

/// A whole document whose root is the mapping `entries`. With `gap`, top-level entries are
/// separated by an empty line (Bruno's layout of item files).
pub(crate) fn document_text(entries: &[(&'static str, Val)], gap: bool) -> String {
    let mut out = String::new();
    for (n, (k, v)) in entries.iter().enumerate() {
        if n > 0 && gap {
            out.push('\n');
        }
        out.push_str(&entry_text(k, v, 0));
        out.push('\n');
    }
    out
}

/// Source text written as is; see the module docs.
fn verbatim(raw: String) -> Node {
    Node::Scalar(Scalar {
        value: String::new(),
        style: Style::Plain,
        raw,
        owner_indent: 0,
        in_flow: false,
        is_key: false,
        comment_after: false,
    })
}

fn with_newline(text: String, nl: &str) -> String {
    if nl == "\n" {
        text
    } else {
        text.replace('\n', nl)
    }
}

/// Re-parses a document after edits, so verbatim nodes become a navigable tree again.
pub(crate) fn reparse(doc: &Document) -> Result<Document, String> {
    parse(&doc.to_text()).map_err(|e| format!("edit produced invalid YAML: {e}"))
}

/// Rank of `key` in a canonical key order; unknown keys sort last.
fn rank(order: &[&str], key: &str) -> usize {
    order.iter().position(|k| *k == key).unwrap_or(order.len())
}

impl Node {
    /// The node as a block mapping that can be edited entry by entry.
    pub fn block_map_mut(&mut self) -> Option<&mut Map> {
        match self {
            Node::Map(m) if !m.flow => Some(m),
            _ => None,
        }
    }

    /// The node as a block sequence that can be edited item by item.
    pub fn block_seq_mut(&mut self) -> Option<&mut super::Seq> {
        match self {
            Node::Seq(s) if !s.flow => Some(s),
            _ => None,
        }
    }
}

impl Scalar {
    /// Replaces the scalar with a plain one written as `raw` (a boolean or a number).
    pub fn set_plain(&mut self, raw: &str) {
        if self.style == Style::Plain && self.raw == raw {
            return;
        }
        self.raw = raw.to_owned();
        self.value = raw.to_owned();
        self.style = Style::Plain;
    }
}

impl Map {
    pub fn index(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.key.value == key)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Sets `key` to `v`. An existing scalar keeps its position and, for strings, the
    /// rendering rules of [`Scalar::set_str`]; anything else is replaced by a rendering of
    /// `v`. A missing key is inserted before the first entry that comes later in `order`
    /// (unknown keys count as later). With `gap`, an inserted entry is preceded by an empty
    /// line. `nl` is the document's line break.
    pub fn put(&mut self, key: &'static str, v: &Val, order: &[&str], gap: bool, nl: &str) {
        let indent = self.indent;
        if let Some(i) = self.index(key) {
            let entry = &mut self.entries[i];
            match (v, &mut entry.value) {
                // An empty value (`key:`) has no room for text after the separator.
                (Val::Str(s), Node::Scalar(scalar)) if !scalar.raw.is_empty() => {
                    scalar.set_str(s, nl)
                }
                (Val::Plain(p), Node::Scalar(scalar)) if !scalar.raw.is_empty() => {
                    scalar.set_plain(p)
                }
                _ => {
                    let (sep, raw) = entry_value(v, indent);
                    entry.sep = sep;
                    entry.value = verbatim(with_newline(raw, nl));
                }
            }
            return;
        }
        let (sep, raw) = entry_value(v, indent);
        let r = rank(order, key);
        let at = self
            .entries
            .iter()
            .position(|e| rank(order, &e.key.value) > r)
            .unwrap_or(self.entries.len());
        let mut lead = String::from(nl);
        if gap {
            lead.push_str(nl);
        }
        lead.push_str(&spaces(indent));
        let mut entry = Entry {
            lead,
            key: Scalar {
                value: key.to_owned(),
                style: Style::Plain,
                raw: key.to_owned(),
                owner_indent: indent as isize,
                in_flow: false,
                is_key: true,
                comment_after: false,
            },
            sep,
            value: verbatim(with_newline(raw, nl)),
        };
        if at == 0 && !self.entries.is_empty() {
            // The first entry's lead belongs to the parent (glue after `key:` or the
            // document head); the new first entry takes it over.
            std::mem::swap(&mut entry.lead, &mut self.entries[0].lead);
        }
        self.entries.insert(at, entry);
    }

    /// Removes `key`. Returns whether it was there. The caller removes a mapping that
    /// becomes empty from its parent (an empty block mapping cannot be written).
    pub fn remove(&mut self, key: &str) -> bool {
        let Some(i) = self.index(key) else {
            return false;
        };
        let removed = self.entries.remove(i);
        if i == 0
            && let Some(next) = self.entries.first_mut()
        {
            next.lead = removed.lead;
        }
        true
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        let i = self.index(key)?;
        Some(&self.entries[i].value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        let i = self.index(key)?;
        Some(&mut self.entries[i].value)
    }
}

/// What to do with the items of a block sequence that is being rewritten.
pub(crate) enum ItemOp<'v> {
    /// Keep the existing item at this index.
    Keep(usize),
    /// Keep the existing item at this index, but let the caller edit it in place.
    Edit(usize),
    /// A new item rendered from the value.
    New(&'v Val),
}

impl super::Seq {
    /// Rebuilds the item list from `ops`. Returns the indices of the resulting items that
    /// the caller asked to edit, as (new position, old index). Kept items keep their source
    /// text, comments included.
    pub fn rebuild(&mut self, ops: &[ItemOp<'_>], nl: &str) -> Vec<usize> {
        let indent = self.indent;
        let first_lead = self.items.first().map(|i| i.lead.clone());
        let mut old: Vec<Option<Item>> = std::mem::take(&mut self.items)
            .into_iter()
            .map(Some)
            .collect();
        let fresh_lead = format!("{nl}{}", spaces(indent));
        let mut edits = Vec::new();
        for op in ops {
            let item = match op {
                ItemOp::Keep(i) | ItemOp::Edit(i) => {
                    if matches!(op, ItemOp::Edit(_)) {
                        edits.push(self.items.len());
                    }
                    let mut item = old[*i].take().expect("each old item used once");
                    if *i == 0 {
                        item.lead = fresh_lead.clone();
                    }
                    item
                }
                ItemOp::New(v) => Item {
                    lead: fresh_lead.clone(),
                    dash: String::new(),
                    value: verbatim(with_newline(item_text(v, indent), nl)),
                },
            };
            self.items.push(item);
        }
        if let (Some(first), Some(lead)) = (self.items.first_mut(), first_lead) {
            first.lead = lead;
        }
        edits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited(src: &str, f: impl FnOnce(&mut Document)) -> String {
        let mut doc = parse(src).unwrap();
        f(&mut doc);
        let text = doc.to_text();
        reparse(&doc).unwrap_or_else(|e| panic!("{e}\n{text}"));
        text
    }

    #[test]
    fn inserts_in_canonical_order() {
        let order = ["a", "b", "c"];
        let out = edited("a: 1\nc: 3\n", |d| {
            d.root
                .block_map_mut()
                .unwrap()
                .put("b", &Val::str("x"), &order, false, "\n")
        });
        assert_eq!(out, "a: 1\nb: x\nc: 3\n");
        let out = edited("# head\nb: 1\n", |d| {
            d.root
                .block_map_mut()
                .unwrap()
                .put("a", &Val::str("x"), &order, true, "\n")
        });
        assert_eq!(out, "# head\na: x\n\nb: 1\n");
    }

    #[test]
    fn renders_nested_values_like_bruno() {
        let v = Val::Map(vec![
            ("method", Val::str("GET")),
            ("url", Val::str("{{host}}/a")),
            (
                "headers",
                Val::Seq(vec![Val::Map(vec![
                    ("name", Val::str("A")),
                    ("value", Val::str("x\ny")),
                ])]),
            ),
        ]);
        let out = edited("info:\n  name: a\n", |d| {
            d.root
                .block_map_mut()
                .unwrap()
                .put("http", &v, &["info", "http"], true, "\n")
        });
        assert_eq!(
            out,
            "info:\n  name: a\n\nhttp:\n  method: GET\n  url: \"{{host}}/a\"\n  headers:\n    - name: A\n      value: |-\n        x\n        y\n"
        );
    }

    #[test]
    fn removes_first_entry_and_keeps_parent_glue() {
        let out = edited("x:\n  a: 1\n  b: 2\n", |d| {
            let x = d.root.block_map_mut().unwrap().get_mut("x").unwrap();
            assert!(x.block_map_mut().unwrap().remove("a"));
        });
        assert_eq!(out, "x:\n  b: 2\n");
    }

    #[test]
    fn rebuilds_sequences_keeping_items() {
        let src = "l:\n  - a: 1\n  # two\n  - a: 2\n  - a: 3\n";
        let new = Val::Map(vec![("a", Val::Plain("4".into()))]);
        let out = edited(src, |d| {
            let l = d.root.block_map_mut().unwrap().get_mut("l").unwrap();
            let seq = l.block_seq_mut().unwrap();
            seq.rebuild(&[ItemOp::Keep(0), ItemOp::New(&new), ItemOp::Keep(1)], "\n");
        });
        // A comment line stays with the item below it.
        assert_eq!(out, "l:\n  - a: 1\n  - a: 4\n  # two\n  - a: 2\n");
    }

    #[test]
    fn keeps_crlf() {
        let out = edited("a: 1\r\n", |d| {
            d.root.block_map_mut().unwrap().put(
                "b",
                &Val::Seq(vec![Val::str("x")]),
                &[],
                false,
                "\r\n",
            )
        });
        assert_eq!(out, "a: 1\r\nb:\r\n  - x\r\n");
    }
}
