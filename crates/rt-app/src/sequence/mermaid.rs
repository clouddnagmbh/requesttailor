// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Mermaid text of a [`SequenceModel`] (`sequenceDiagram`) and its Markdown block.
//!
//! Mermaid reads `;` as the end of a statement, `#` as the start of an entity, and `:` and
//! `%%` have meaning in places, so every label goes through [`escape`]: those characters
//! become entity codes (`#59;`, `#35;`, `#58;`, `#37;`), line breaks and control characters
//! become spaces, angle brackets and backticks become entities, and a label is cut at
//! [`MAX_LABEL`] characters. Participants are written as `P0`, `P1`, … with the readable
//! label as alias, so hosts with ports and agent names never break the syntax. The text is
//! cut before [`MAX_TEXT`] bytes (open groups are closed).

use std::collections::HashMap;
use std::fmt::Write as _;

use super::{
    Group, MAX_LABEL, MAX_TEXT, Message, MessageKind, NoteAnchor, SequenceModel, SequenceOptions,
    SequenceText,
};

/// A label as Mermaid can read it.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (count, c) in text.trim().chars().enumerate() {
        if count >= MAX_LABEL {
            out.push('…');
            break;
        }
        match c {
            ';' => out.push_str("#59;"),
            '#' => out.push_str("#35;"),
            ':' => out.push_str("#58;"),
            '%' => out.push_str("#37;"),
            '<' => out.push_str("#lt;"),
            '>' => out.push_str("#gt;"),
            '`' => out.push_str("#96;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    if out.trim().is_empty() {
        "-".into()
    } else {
        out
    }
}

fn group_keyword(kind: &str) -> &'static str {
    match kind {
        "rect" => "rect",
        "alt" => "alt",
        _ => "loop",
    }
}

fn open_group(out: &mut String, indent: usize, group: &Group) {
    let pad = "    ".repeat(indent);
    match group_keyword(&group.kind) {
        "rect" => {
            let _ = writeln!(out, "{pad}rect rgb(235, 240, 255)");
            if !group.label.is_empty() {
                let _ = writeln!(out, "{pad}    %% {}", escape(&group.label));
            }
        }
        keyword => {
            let _ = writeln!(out, "{pad}{keyword} {}", escape(&group.label));
        }
    }
}

fn arrow(m: &Message) -> &'static str {
    match m.kind {
        MessageKind::Request => "->>",
        MessageKind::Response => "-->>",
    }
}

/// The Mermaid text and its Markdown block.
pub fn render(model: &SequenceModel, _options: &SequenceOptions) -> SequenceText {
    let mermaid = mermaid_text(model);
    let markdown = to_markdown(&mermaid);
    SequenceText { mermaid, markdown }
}

/// A fenced `mermaid` block (GitHub and GitLab draw it in descriptions and comments).
pub fn to_markdown(mermaid: &str) -> String {
    format!("```mermaid\n{}\n```\n", mermaid.trim_end())
}

fn mermaid_text(model: &SequenceModel) -> String {
    let mut out = String::from("sequenceDiagram\n");
    let mut alias: HashMap<&str, String> = HashMap::new();
    for (i, p) in model.participants.iter().enumerate() {
        let name = format!("P{i}");
        let _ = writeln!(out, "    participant {name} as {}", escape(&p.label));
        alias.insert(p.id.as_str(), name);
    }
    let name_of = |id: &str| alias.get(id).cloned().unwrap_or_else(|| "P0".to_owned());
    let mut groups: Vec<&Group> = model
        .groups
        .iter()
        .filter(|g| g.from <= g.to && g.to < model.messages.len())
        .collect();
    // Outer groups first at the same start.
    groups.sort_by(|a, b| a.from.cmp(&b.from).then(b.to.cmp(&a.to)));
    let mut open: Vec<&Group> = Vec::new();
    let mut next_group = 0;
    let mut cut = false;
    for (i, m) in model.messages.iter().enumerate() {
        if out.len() >= MAX_TEXT - 4096 {
            cut = true;
            break;
        }
        while next_group < groups.len() && groups[next_group].from == i {
            open_group(&mut out, open.len() + 1, groups[next_group]);
            open.push(groups[next_group]);
            next_group += 1;
        }
        let pad = "    ".repeat(open.len() + 1);
        let _ = writeln!(
            out,
            "{pad}{}{}{}: {}",
            name_of(&m.from),
            arrow(m),
            name_of(&m.to),
            escape(&m.label)
        );
        for note in model.notes.iter().filter(|n| n.after == i) {
            match &note.anchor {
                NoteAnchor::Over { participant } => {
                    let _ = writeln!(
                        out,
                        "{pad}Note over {}: {}",
                        name_of(participant),
                        escape(&note.text)
                    );
                }
                NoteAnchor::Between { from, to } => {
                    let _ = writeln!(
                        out,
                        "{pad}Note over {},{}: {}",
                        name_of(from),
                        name_of(to),
                        escape(&note.text)
                    );
                }
            }
        }
        while open.last().is_some_and(|g| g.to == i) {
            open.pop();
            let _ = writeln!(out, "{}end", "    ".repeat(open.len() + 1));
        }
    }
    if cut {
        let _ = writeln!(out, "    %% cut: the text reached its size limit");
    }
    while !open.is_empty() {
        open.pop();
        let _ = writeln!(out, "{}end", "    ".repeat(open.len() + 1));
    }
    out
}
