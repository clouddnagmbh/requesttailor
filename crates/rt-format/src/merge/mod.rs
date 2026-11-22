// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Three-way merge of a collection on the model (wave 12): a conflict only where both sides
//! changed the same field ([`crate::diff::FieldPath`]) differently; everything else merges.
//!
//! [`merge`] is the one merge for the app's pull (desktop and web), `rt merge-driver` (Git's
//! merge driver, so the Git command line merges the same way) and later rebase and
//! cherry-pick. Files neither side changed stay byte-identical; a file only one side changed
//! is taken from that side byte-identical; a file both sides changed is written with the
//! format-preserving writer, so untouched parts of it stay as they were.
//!
//! Conflicts are not stored anywhere: after a stopped merge Git's index holds base, ours and
//! theirs (stages 1–3), and the app recomputes [`MergeResult::conflicts`] from them.
//!
//! Owner: wave 12 interface; implementation strand B (`fields.rs`: the model, field by field;
//! `file.rs`: one file; `text.rs`: the line merge; `markers.rs`: comments for the working tree).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::diff::{ChangeTarget, DiffValue, FieldPath};
use crate::{FileSet, FormatError};

mod fields;
mod file;
mod markers;
mod text;

use file::Version;

/// Both sides changed the same field differently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldConflict {
    pub target: ChangeTarget,
    /// The target's file (ours; theirs if ours removed it).
    pub file: String,
    pub field: FieldPath,
    /// `None` where the field does not exist on that side.
    pub base: Option<DiffValue>,
    pub ours: Option<DiffValue>,
    pub theirs: Option<DiffValue>,
}

/// The result of a merge.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MergeResult {
    /// The merged files. Conflicted fields and files keep "ours" until resolved.
    pub files: FileSet,
    /// Field conflicts, in the order of [`crate::diff::CollectionDiff::changes`].
    pub conflicts: Vec<FieldConflict>,
    /// Files both sides changed that could not be merged on the model (not a collection file,
    /// not parsable on a side, a change the model cannot express); `files` holds ours.
    pub text_conflicts: Vec<String>,
}

impl MergeResult {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty() && self.text_conflicts.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Ours,
    Theirs,
}

/// How to resolve one conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ConflictChoice {
    /// One field of a [`FieldConflict`].
    Field {
        target: ChangeTarget,
        field: FieldPath,
        side: Side,
    },
    /// A whole file of [`MergeResult::text_conflicts`].
    File { path: String, side: Side },
}

/// Merges `ours` and `theirs`, both descendants of `base`. An empty `base` means the sides
/// share no ancestor (everything both added is compared field by field).
///
/// Files are merged one by one ([`merge_with`] has the rules); a file one side moved (renamed
/// or put in another folder: same bytes, or the same request or environment apart from its
/// name) while the other side changed it is merged into the new path. Never fails today; the
/// `Result` is for sides that cannot be read at all.
pub fn merge(base: &FileSet, ours: &FileSet, theirs: &FileSet) -> Result<MergeResult, FormatError> {
    merge_with(base, ours, theirs, &[])
}

/// [`merge`] with some conflicts decided by `choices`; conflicts without a choice stay in the
/// result. A [`ConflictChoice::File`] takes that side's file as a whole, also for a file with
/// field conflicts.
///
/// Per file: unchanged on a side → the other side's bytes. Changed on both: a collection file
/// (request, folder, collection, environment) is merged on the model and written into one
/// side's file with the format-preserving writer, so everything the model does not hold
/// stays (see `file.rs` for which side). Where that is not possible (not a collection file,
/// not parsable, a change the writer cannot make), the file is merged line by line like
/// Git's text merge; only overlapping line changes make it a text conflict. Deleted on one
/// side and changed on the other is a text conflict too.
pub fn merge_with(
    base: &FileSet,
    ours: &FileSet,
    theirs: &FileSet,
    choices: &[ConflictChoice],
) -> Result<MergeResult, FormatError> {
    Ok(merge_marked(base, ours, theirs, choices)?.0)
}

/// [`merge_with`], plus what to put in the working tree for each conflicted file: the merged
/// file with a comment above each conflicted field (field conflicts), or the line merge with
/// Git's conflict markers (text conflicts; absent if a side deleted the file). Used where a
/// merge stops for the user (`rt merge-driver`, the app's pull).
pub fn merge_marked(
    base: &FileSet,
    ours: &FileSet,
    theirs: &FileSet,
    choices: &[ConflictChoice],
) -> Result<(MergeResult, BTreeMap<String, Vec<u8>>), FormatError> {
    fn v<'a>(set: &'a FileSet, path: &str) -> Option<Version<'a>> {
        set.files.get_key_value(path).map(|(p, b)| Version {
            path: p.as_str(),
            bytes: b.as_slice(),
        })
    }
    let moves = moved(base, ours, theirs);
    let mut jobs: Vec<Job<'_>> = Vec::new();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for m in &moves {
        let (o, t) = match m.side {
            Side::Ours => (v(ours, &m.to), v(theirs, &m.from)),
            Side::Theirs => (v(ours, &m.from), v(theirs, &m.to)),
        };
        jobs.push((m.to.clone(), v(base, &m.from), o, t));
        done.insert(&m.from);
        done.insert(&m.to);
    }
    let paths: BTreeSet<&String> = base
        .files
        .keys()
        .chain(ours.files.keys())
        .chain(theirs.files.keys())
        .collect();
    for p in paths {
        if !done.contains(p.as_str()) {
            jobs.push((p.clone(), v(base, p), v(ours, p), v(theirs, p)));
        }
    }
    let mut out = MergeResult::default();
    let mut marked = BTreeMap::new();
    let mut conflicts: Vec<(String, FieldConflict)> = Vec::new();
    for (path, b, o, t) in jobs {
        // Only files of this collection are merged on the model.
        let owned = |set: &FileSet| crate::owned(&path, set);
        let model = owned(ours) || owned(theirs);
        let (target, m) = if model {
            file::merge_file(&path, b, o, t, choices)
        } else {
            file::merge_text(&path, b, o, t, choices)
        };
        if let Some(bytes) = &m.bytes {
            out.files.files.insert(path.clone(), bytes.clone());
        }
        if m.text_conflict {
            out.text_conflicts.push(path.clone());
            if let Some(text) = m.marked {
                marked.insert(path.clone(), text.into_bytes());
            }
        }
        let file_conflicts: Vec<FieldConflict> = m
            .conflicts
            .iter()
            .map(|c| {
                let [base, ours, theirs] = c.values();
                FieldConflict {
                    target: target.clone(),
                    file: path.clone(),
                    field: c.field.clone(),
                    base,
                    ours,
                    theirs,
                }
            })
            .collect();
        if !file_conflicts.is_empty()
            && let Some(bytes) = &m.bytes
        {
            marked.insert(path.clone(), markers::mark(&path, bytes, &file_conflicts));
        }
        conflicts.extend(file_conflicts.into_iter().map(|c| (path.clone(), c)));
    }
    // Tree order: the collection, folders and requests depth first, then environments.
    let order = |p: &str| match file::kind(p) {
        file::Kind::Collection => (0, String::new()),
        file::Kind::Environment => (2, p.to_owned()),
        file::Kind::Folder(dir) => (1, format!("{dir}/")),
        _ => (1, p.to_owned()),
    };
    conflicts.sort_by_key(|(p, _)| order(p));
    out.conflicts = conflicts.into_iter().map(|(_, c)| c).collect();
    out.text_conflicts.sort_by_key(|p| order(p));
    Ok((out, marked))
}

/// One file to merge: result path, base, ours, theirs.
type Job<'a> = (
    String,
    Option<Version<'a>>,
    Option<Version<'a>>,
    Option<Version<'a>>,
);

/// A file one side moved while the other side changed it at the old path.
struct Move {
    side: Side,
    from: String,
    to: String,
}

fn moved(base: &FileSet, ours: &FileSet, theirs: &FileSet) -> Vec<Move> {
    let mut out: Vec<Move> = Vec::new();
    for (side, mover, other) in [(Side::Ours, ours, theirs), (Side::Theirs, theirs, ours)] {
        for (from, bytes) in &base.files {
            let changed_by_other = other.files.get(from).is_some_and(|o| o != bytes);
            if mover.files.contains_key(from) || !changed_by_other {
                continue;
            }
            let old = Version { path: from, bytes };
            let candidates = mover.files.iter().filter(|(to, b)| {
                !base.files.contains_key(*to)
                    && !other.files.contains_key(*to)
                    && !out.iter().any(|m| &m.to == *to)
                    && file::same_content(old, Version { path: to, bytes: b })
            });
            // Prefer a file of the same name (moved to another folder).
            let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_owned();
            let all: Vec<&String> = candidates.map(|(p, _)| p).collect();
            let pick = all
                .iter()
                .find(|p| name(p) == name(from))
                .or_else(|| all.first())
                .map(|p| (*p).clone());
            if let Some(to) = pick {
                out.push(Move {
                    side,
                    from: from.clone(),
                    to,
                });
            }
        }
    }
    out
}

/// The merge of [`merge`] with every conflict decided by `choices`. Fails with
/// [`FormatError::Invalid`] if a conflict is left without a choice.
pub fn resolve(
    base: &FileSet,
    ours: &FileSet,
    theirs: &FileSet,
    choices: &[ConflictChoice],
) -> Result<FileSet, FormatError> {
    let result = merge_with(base, ours, theirs, choices)?;
    if let Some(c) = result.conflicts.first() {
        return Err(FormatError::Invalid {
            path: c.file.clone(),
            message: format!("the conflict in {} has no resolution", c.field.join(" / ")),
        });
    }
    if let Some(path) = result.text_conflicts.first() {
        return Err(FormatError::Invalid {
            path: path.clone(),
            message: "the conflict has no resolution".into(),
        });
    }
    Ok(result.files)
}

/// The outcome of [`merge_file`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileOutcome {
    /// What to write: the merged file, with a comment above each conflicted field; for a
    /// text conflict the line merge with Git's conflict markers (ours if not text). `None`:
    /// the merge deletes the file.
    pub bytes: Option<Vec<u8>>,
    pub conflicts: Vec<FieldConflict>,
    /// The file could not be merged on the model or line by line.
    pub text_conflict: bool,
}

/// Merges one file of a collection on its own (`rt merge-driver`: Git hands over one file at
/// a time). `path` is relative to the collection root (it decides what kind of file it is);
/// `None` for a file that is not in a collection, which is merged line by line.
pub fn merge_file(
    path: Option<&str>,
    base: Option<&[u8]>,
    ours: Option<&[u8]>,
    theirs: Option<&[u8]>,
) -> FileOutcome {
    let p = path.unwrap_or("");
    let [b, o, t] = [base, ours, theirs].map(|v| v.map(|bytes| Version { path: p, bytes }));
    let (target, m) = match path {
        Some(_) => file::merge_file(p, b, o, t, &[]),
        None => file::merge_text(p, b, o, t, &[]),
    };
    let conflicts: Vec<FieldConflict> = m
        .conflicts
        .iter()
        .map(|c| {
            let [base, ours, theirs] = c.values();
            FieldConflict {
                target: target.clone(),
                file: p.to_owned(),
                field: c.field.clone(),
                base,
                ours,
                theirs,
            }
        })
        .collect();
    let bytes = match (&m.marked, &m.bytes) {
        (Some(text), _) => Some(text.clone().into_bytes()),
        (None, Some(bytes)) if !conflicts.is_empty() => Some(markers::mark(p, bytes, &conflicts)),
        (None, bytes) => bytes.clone(),
    };
    FileOutcome {
        bytes,
        conflicts,
        text_conflict: m.text_conflict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(files: &[(&str, &str)]) -> FileSet {
        FileSet {
            files: files
                .iter()
                .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
                .collect(),
        }
    }

    #[test]
    fn files_merge_from_the_side_that_changed_them() {
        let base = set(&[("a.yml", "a"), ("b.yml", "b"), ("c.yml", "c")]);
        let ours = set(&[("a.yml", "A"), ("b.yml", "b"), ("c.yml", "c1")]);
        let theirs = set(&[
            ("a.yml", "a"),
            ("b.yml", "B"),
            ("c.yml", "c2"),
            ("d.yml", "d"),
        ]);
        let r = merge(&base, &ours, &theirs).unwrap();
        assert_eq!(r.text_conflicts, vec!["c.yml".to_string()]);
        assert_eq!(r.files.files["a.yml"], b"A");
        assert_eq!(r.files.files["b.yml"], b"B");
        assert_eq!(r.files.files["c.yml"], b"c1");
        assert_eq!(r.files.files["d.yml"], b"d");
        let choice = ConflictChoice::File {
            path: "c.yml".into(),
            side: Side::Theirs,
        };
        let files = resolve(&base, &ours, &theirs, &[choice]).unwrap();
        assert_eq!(files.files["c.yml"], b"c2");
        assert!(resolve(&base, &ours, &theirs, &[]).is_err());
    }
}
