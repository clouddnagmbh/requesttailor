// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Conflict markers per field, for the working tree after a stopped merge (`rt merge-driver`,
//! the app's pull): a YAML comment above each conflicted field names the field and theirs'
//! value; the file itself keeps ours and stays a valid collection file. A field that cannot
//! be found (or a comment that would change what the file reads as) is noted at the top.

use super::FieldConflict;
use super::file::{Kind, kind, read_model};
use crate::diff::DiffValue;
use crate::yaml::{self, Document, Node};

/// One step from a node to a child.
enum Step<'a> {
    Key(&'a str),
    /// The item of a sequence whose mapping has `key: value`.
    Item(&'a str, String),
}

#[derive(Clone, Copy)]
enum At {
    Entry(usize),
    Item(usize),
}

fn setting_key(field: &str) -> &str {
    match field {
        "timeout_ms" => "timeout",
        "follow_redirects" => "followRedirects",
        "max_redirects" => "maxRedirects",
        "encode_url" => "encodeUrl",
        other => other,
    }
}

/// Request-default fields (`folder.yml`, `opencollection.yml`) under `request`.
fn defaults_steps<'a>(rest: &'a [String], out: &mut Vec<Step<'a>>) {
    out.push(Step::Key("request"));
    match rest {
        [f, name, ..] if f == "headers" => {
            out.push(Step::Key("headers"));
            out.push(Step::Item("name", name.clone()));
        }
        [f, name, ..] if f == "vars" => {
            out.push(Step::Key("variables"));
            out.push(Step::Item("name", name.clone()));
        }
        [f, phase, ..] if f == "scripts" => {
            out.push(Step::Key("scripts"));
            out.push(Step::Item("type", phase.clone()));
        }
        [f, ..] if f == "settings" => out.push(Step::Key("settings")),
        [f, ..] => out.push(Step::Key(f)),
        [] => {}
    }
}

fn steps<'a>(kind: &Kind, root: &Node, field: &'a [String]) -> Vec<Step<'a>> {
    let mut out = Vec::new();
    let first = field.first().map(String::as_str).unwrap_or_default();
    match kind {
        Kind::Request => {
            let proto = ["http", "graphql", "grpc", "websocket"]
                .into_iter()
                .find(|k| root.get(k).is_some())
                .unwrap_or("http");
            match (first, &field[field.len().min(1)..]) {
                ("name" | "seq", _) => {
                    out.push(Step::Key("info"));
                    out.push(Step::Key(first));
                }
                ("headers", [name, ..]) => {
                    out.push(Step::Key(proto));
                    out.push(Step::Key(if proto == "grpc" {
                        "metadata"
                    } else {
                        "headers"
                    }));
                    out.push(Step::Item("name", name.clone()));
                }
                ("params", [_, name, ..]) => {
                    out.push(Step::Key(proto));
                    out.push(Step::Key("params"));
                    out.push(Step::Item("name", name.clone()));
                }
                ("vars", [name, ..]) => {
                    out.push(Step::Key("runtime"));
                    out.push(Step::Key("variables"));
                    out.push(Step::Item("name", name.clone()));
                }
                ("scripts", [phase, ..]) => {
                    out.push(Step::Key("runtime"));
                    out.push(Step::Key("scripts"));
                    out.push(Step::Item("type", phase.clone()));
                }
                ("assertions", [expr, ..]) => {
                    out.push(Step::Key("runtime"));
                    out.push(Step::Key("assertions"));
                    out.push(Step::Item("expression", expr.clone()));
                }
                ("actions", _) => {
                    out.push(Step::Key("runtime"));
                    out.push(Step::Key("actions"));
                }
                ("settings", [f, ..]) => {
                    out.push(Step::Key("settings"));
                    out.push(Step::Key(setting_key(f)));
                }
                ("protocol", _) => out.push(Step::Key(proto)),
                (f, _) => {
                    out.push(Step::Key(proto));
                    out.push(Step::Key(f));
                }
            }
        }
        Kind::Environment => match field {
            [f, name, ..] if f == "variables" => {
                out.push(Step::Key("variables"));
                out.push(Step::Item("name", name.clone()));
            }
            [f, ..] => out.push(Step::Key(f)),
            [] => {}
        },
        Kind::Folder(_) | Kind::Collection => match field {
            [f, ..] if f == "name" || f == "seq" => {
                out.push(Step::Key("info"));
                out.push(Step::Key(f));
            }
            [f, rest @ ..] if f == "defaults" => defaults_steps(rest, &mut out),
            [f, name, ..] if f == "environments" => {
                out.push(Step::Key("config"));
                out.push(Step::Key("environments"));
                out.push(Step::Item("name", name.clone()));
            }
            [f, ..] => out.push(Step::Key(if f == "config" { "config" } else { f })),
            [] => {}
        },
        Kind::Other => {}
    }
    out
}

/// The way down to the deepest node of `steps` that exists.
fn locate(root: &Node, steps: &[Step<'_>]) -> Vec<At> {
    let mut at = Vec::new();
    let mut node = root;
    for step in steps {
        let found = match (step, node) {
            (Step::Key(k), Node::Map(m)) => m
                .entries
                .iter()
                .position(|e| e.key.value == *k)
                .map(|i| (At::Entry(i), &m.entries[i].value)),
            (Step::Item(k, v), Node::Seq(s)) => s
                .items
                .iter()
                .position(|i| i.value.get(k).and_then(Node::text) == Some(v.as_str()))
                .map(|i| (At::Item(i), &s.items[i].value)),
            _ => None,
        };
        match found {
            Some((a, next)) => {
                at.push(a);
                node = next;
            }
            None => break,
        }
    }
    at
}

fn lead_mut<'a>(root: &'a mut Node, at: &[At]) -> Option<&'a mut String> {
    let (last, path) = at.split_last()?;
    let mut node = root;
    for a in path {
        node = match (a, node) {
            (At::Entry(i), Node::Map(m)) => &mut m.entries.get_mut(*i)?.value,
            (At::Item(i), Node::Seq(s)) => &mut s.items.get_mut(*i)?.value,
            _ => return None,
        };
    }
    match (last, node) {
        (At::Entry(i), Node::Map(m)) => match m.flow {
            true => None,
            false => Some(&mut m.entries.get_mut(*i)?.lead),
        },
        (At::Item(i), Node::Seq(s)) => match s.flow {
            true => None,
            false => Some(&mut s.items.get_mut(*i)?.lead),
        },
        _ => None,
    }
}

fn value_text(v: &Option<DiffValue>) -> String {
    match v {
        None => "(removed)".into(),
        Some(DiffValue::Secret) => "(a secret value)".into(),
        Some(DiffValue::Text { text }) => text.clone(),
        Some(DiffValue::Json { value }) => value.to_string(),
    }
}

/// The comment lines for one conflict (without `#`).
fn comment(c: &FieldConflict) -> Vec<String> {
    let field = c.field.join(" / ");
    let theirs = value_text(&c.theirs);
    let mut lines = Vec::new();
    if theirs.contains('\n') {
        lines.push(format!(
            "rt merge-driver: conflict in {field} (ours kept below). Theirs:"
        ));
        lines.extend(theirs.lines().map(|l| format!("  {l}")));
    } else {
        lines.push(format!(
            "rt merge-driver: conflict in {field} (ours kept below). Theirs: {theirs}"
        ));
    }
    lines
}

/// Puts comment lines in front of the entry with this `lead`, at its indentation.
fn insert(lead: &mut String, lines: &[String], nl: &str) -> bool {
    let Some(pos) = lead.rfind('\n') else {
        return false;
    };
    let indent = &lead[pos + 1..];
    if !indent.chars().all(|c| c == ' ') {
        return false;
    }
    let indent = indent.to_owned();
    for l in lines {
        lead.push_str("# ");
        lead.push_str(l);
        lead.push_str(nl);
        lead.push_str(&indent);
    }
    true
}

fn at_top(doc: &mut Document, lines: &[String]) {
    let nl = doc.newline;
    let mut text = String::new();
    for l in lines {
        text.push_str("# ");
        text.push_str(l);
        text.push_str(nl);
    }
    doc.head = format!("{text}{}", doc.head);
}

/// `bytes` (the file at `path`, as merged: ours in conflicted fields) with a comment for
/// each of `conflicts`.
pub(crate) fn mark(path: &str, bytes: &[u8], conflicts: &[FieldConflict]) -> Vec<u8> {
    let Some(doc) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| yaml::parse(s).ok())
    else {
        return bytes.to_vec();
    };
    let k = kind(path);
    let model = read_model(&k, path, &doc);
    let mut marked = doc.clone();
    let nl = doc.newline;
    let mut top = Vec::new();
    for c in conflicts {
        let lines = comment(c);
        let steps = steps(&k, &doc.root, &c.field);
        let mut at = locate(&doc.root, &steps);
        let mut done = false;
        while !at.is_empty() && !done {
            if let Some(lead) = lead_mut(&mut marked.root, &at) {
                done = insert(lead, &lines, nl);
            }
            at.pop();
        }
        if !done {
            top.extend(lines);
        }
    }
    // Comments must not change what the file reads as.
    let reads_same = |d: &Document| {
        yaml::parse(&d.to_text())
            .ok()
            .is_some_and(|p| read_model(&k, path, &p) == model)
    };
    if !reads_same(&marked) {
        marked = doc.clone();
        top = conflicts.iter().flat_map(comment).collect();
    }
    if !top.is_empty() {
        at_top(&mut marked, &top);
    }
    marked.to_text().into_bytes()
}
