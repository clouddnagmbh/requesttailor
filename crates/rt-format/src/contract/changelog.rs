// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The API changelog in Markdown.

use std::fmt::Write as _;

use super::{SpecChange, SpecChanges};
use crate::diff::{ChangeKind, ChangeTarget, CollectionDiff, FileStatus, ModelChange};

fn line(c: &SpecChange) -> String {
    let mut s = String::from("- ");
    match &c.operation {
        Some(op) => {
            let _ = write!(s, "`{op}`: {}", c.message);
        }
        None => s.push_str(&c.message),
    }
    if !c.requests.is_empty() {
        let list: Vec<String> = c.requests.iter().map(|r| format!("`{r}`")).collect();
        let _ = write!(s, " (affects {})", list.join(", "));
    }
    s.push('\n');
    s
}

fn target(change: &ModelChange) -> String {
    match &change.target {
        ChangeTarget::Collection => "collection".into(),
        ChangeTarget::Folder { path } => format!("folder `{path}`"),
        ChangeTarget::Request { path } => format!("request `{path}`"),
        ChangeTarget::Environment { name } => format!("environment `{name}`"),
    }
}

pub(super) fn render(title: &str, spec: &SpecChanges, collection: &CollectionDiff) -> String {
    let mut out = format!("# {title}\n\n");
    let breaking: Vec<&SpecChange> = spec.changes.iter().filter(|c| c.breaking).collect();
    let other: Vec<&SpecChange> = spec.changes.iter().filter(|c| !c.breaking).collect();

    out.push_str("## Breaking changes\n\n");
    if breaking.is_empty() {
        out.push_str("None.\n");
    }
    for c in &breaking {
        out.push_str(&line(c));
    }
    if !other.is_empty() {
        out.push_str("\n## Other changes\n\n");
        for c in &other {
            out.push_str(&line(c));
        }
    }
    if !collection.changes.is_empty() {
        out.push_str("\n## Collection\n\n");
        for c in &collection.changes {
            let what = match &c.change {
                ChangeKind::Added => "added".to_owned(),
                ChangeKind::Removed => "removed".to_owned(),
                ChangeKind::Modified => "changed".to_owned(),
                ChangeKind::Moved { from } => format!("moved from `{from}`"),
            };
            let fields = if c.fields.is_empty() {
                String::new()
            } else {
                let names: Vec<String> = c.fields.iter().map(|f| f.path.join(".")).collect();
                format!(": {}", names.join(", "))
            };
            let _ = writeln!(out, "- {} {what}{fields}", target(c));
        }
    } else if !collection.files.is_empty() {
        out.push_str("\n## Collection\n\n");
        for f in &collection.files {
            let what = match f.status {
                FileStatus::Added => "added",
                FileStatus::Removed => "removed",
                FileStatus::Modified => "changed",
            };
            let _ = writeln!(out, "- `{}` {what}", f.path);
        }
    }
    out
}
