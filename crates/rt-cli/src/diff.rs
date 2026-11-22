// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt diff [<rev>..<rev> | <rev>] [--json] [<collection>]`: the semantic diff
//! (`rt_app::diffs`) between revisions of the collection, read with `rt-git` without a
//! checkout.
//!
//! - no revision: the saved files against `HEAD`;
//! - `<rev>`: that commit against its first parent (the empty collection for a root commit);
//! - `<rev>..<rev>`: the two trees (`a..` means `a..HEAD`, `..b` means `HEAD..b`).
//!
//! `<collection>` is the collection folder (default: the current folder); an argument that is
//! an existing folder is the collection, anything else is a revision. Output is text, or the
//! `CollectionDiff` as JSON with `--json`. Secret values never appear. Exit code 0 (also
//! when there are differences), 2 for bad arguments, an unreadable collection or an unknown
//! revision.

use std::io::Write;
use std::path::PathBuf;

use rt_app::diffs::{
    ChangeKind, ChangeTarget, ChangedFile, CollectionDiff, DiffValue, FieldChange, FileStatus,
};
use rt_format::FileSet;
use rt_git::{Options, Repo};

/// The line in `rt --help`'s "Usage:" list.
pub const USAGE: &str = "  rt diff [<rev>..<rev> | <rev>] [--json] [<collection>]\n";

/// The section in `rt --help`.
pub const HELP: &str = "rt diff shows what changed per request field between two revisions (default:\nthe saved files against HEAD; <rev>: that commit against its parent), as text\nor --json (CollectionDiff). Secret values never appear. An argument that is an\nexisting folder is the collection (default: the current folder).\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The revision argument as typed, if any.
    pub rev: Option<String>,
    pub json: bool,
    pub collection: PathBuf,
}

/// `Ok(None)`: `--help`.
pub fn parse(parser: &mut lexopt::Parser) -> Result<Option<Args>, String> {
    use lexopt::prelude::*;
    let mut rev = None;
    let mut collection = None;
    let mut json = false;
    while let Some(arg) = parser.next().map_err(|e| e.to_string())? {
        match arg {
            Short('h') | Long("help") => return Ok(None),
            Long("json") => json = true,
            Value(v) => {
                let path = PathBuf::from(&v);
                if collection.is_none() && path.is_dir() {
                    collection = Some(path);
                } else if rev.is_none() {
                    rev = Some(v.to_string_lossy().into_owned());
                } else {
                    return Err(format!("unexpected argument {:?}", v.to_string_lossy()));
                }
            }
            other => return Err(other.unexpected().to_string()),
        }
    }
    Ok(Some(Args {
        rev,
        json,
        collection: collection.unwrap_or_else(|| PathBuf::from(".")),
    }))
}

fn tree(repo: &Repo, rev: &str) -> Result<FileSet, String> {
    repo.tree_at(rev)
        .map(|t| t.into_file_set())
        .map_err(|e| e.to_string())
}

/// The two sides of the diff.
fn sides(args: &Args, repo: &Repo) -> Result<(FileSet, FileSet), String> {
    let has_head = repo.rev_parse("HEAD").is_ok();
    match args.rev.as_deref() {
        None => {
            let old = if has_head {
                tree(repo, "HEAD")?
            } else {
                FileSet::default()
            };
            let new = FileSet::from_dir(&args.collection)
                .map_err(|e| format!("cannot read {}: {e}", args.collection.display()))?;
            Ok((old, new))
        }
        Some(spec) => match spec.split_once("..") {
            Some((a, b)) => {
                let a = if a.is_empty() { "HEAD" } else { a };
                let b = if b.is_empty() { "HEAD" } else { b };
                Ok((tree(repo, a)?, tree(repo, b)?))
            }
            None => {
                let new = tree(repo, spec)?;
                let parent = format!("{spec}^");
                // A root commit has no parent: the collection did not exist before it.
                let old = if repo.rev_parse(&parent).is_ok() {
                    tree(repo, &parent)?
                } else {
                    FileSet::default()
                };
                Ok((old, new))
            }
        },
    }
}

pub fn run(args: Args, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let result = (|| {
        let repo =
            Repo::require(&args.collection, &Options::default()).map_err(|e| e.to_string())?;
        let (old, new) = sides(&args, &repo)?;
        rt_app::diffs::diff(&old, &new).map_err(|e| e.to_string())
    })();
    match result {
        Ok(diff) => {
            let text = if args.json {
                serde_json::to_string_pretty(&diff).unwrap_or_default() + "\n"
            } else {
                render(&diff)
            };
            let _ = stdout.write_all(text.as_bytes());
            crate::EXIT_OK
        }
        Err(message) => {
            let _ = writeln!(stderr, "rt diff: {message}");
            crate::EXIT_USAGE
        }
    }
}

fn show(value: &Option<DiffValue>) -> String {
    match value {
        None => "(none)".into(),
        Some(DiffValue::Secret) => "<secret>".into(),
        Some(DiffValue::Json { value }) => value.to_string(),
        Some(DiffValue::Text { text }) => {
            let lines = text.lines().count();
            if lines > 1 {
                let first = text.lines().next().unwrap_or("");
                format!("{first:?} ... ({lines} lines)")
            } else {
                format!("{text:?}")
            }
        }
    }
}

fn field(f: &FieldChange) -> String {
    let at = f.line.map_or(String::new(), |l| format!("  (line {l})"));
    let path = f.path.join(" / ");
    match (&f.old, &f.new) {
        (None, new) => format!("    + {path}: {}{at}\n", show(new)),
        (old, None) => format!("    - {path}: {}{at}\n", show(old)),
        (old, new) => format!("    ~ {path}: {} -> {}{at}\n", show(old), show(new)),
    }
}

fn target(t: &ChangeTarget) -> &'static str {
    match t {
        ChangeTarget::Collection => "collection",
        ChangeTarget::Folder { .. } => "folder",
        ChangeTarget::Request { .. } => "request",
        ChangeTarget::Environment { .. } => "environment",
    }
}

fn not_covered(f: &ChangedFile) -> String {
    let status = match f.status {
        FileStatus::Added => "added",
        FileStatus::Removed => "removed",
        FileStatus::Modified => "modified",
    };
    format!(
        "  ! {} ({status}): differs in ways the model does not show; see `git diff`\n",
        f.path
    )
}

/// The text form of a diff.
pub fn render(diff: &CollectionDiff) -> String {
    if diff.is_empty() {
        return "No changes.\n".into();
    }
    let mut out = String::new();
    for c in &diff.changes {
        let (mark, what) = match &c.change {
            ChangeKind::Added => ("+", "added".to_owned()),
            ChangeKind::Removed => ("-", "removed".to_owned()),
            ChangeKind::Modified => ("~", "modified".to_owned()),
            ChangeKind::Moved { from } => (">", format!("moved from {from}")),
        };
        out.push_str(&format!(
            "{mark} {} {:?} {what}  [{}]\n",
            target(&c.target),
            c.name,
            c.file
        ));
        for f in &c.fields {
            out.push_str(&field(f));
        }
    }
    let uncovered: Vec<_> = diff.files.iter().filter(|f| !f.covered).collect();
    // A modified file with no model change is only listed here.
    if !uncovered.is_empty() {
        out.push_str("Not fully covered:\n");
        for f in uncovered {
            out.push_str(&not_covered(f));
        }
    }
    out
}
