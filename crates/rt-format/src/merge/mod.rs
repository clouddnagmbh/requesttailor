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
//! Owner: wave 12 interface; implementation strand B.

use serde::{Deserialize, Serialize};

use crate::diff::{ChangeTarget, DiffValue, FieldPath};
use crate::{FileSet, FormatError};

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
/// Interface state (wave 12): file level only. A file is taken from the side that changed it;
/// a file both sides changed differently is a text conflict. Strand B merges those on the
/// model.
pub fn merge(base: &FileSet, ours: &FileSet, theirs: &FileSet) -> Result<MergeResult, FormatError> {
    let mut out = MergeResult::default();
    let paths: std::collections::BTreeSet<&String> = base
        .files
        .keys()
        .chain(ours.files.keys())
        .chain(theirs.files.keys())
        .collect();
    for path in paths {
        let (b, o, t) = (
            base.files.get(path),
            ours.files.get(path),
            theirs.files.get(path),
        );
        let take = if o == t || b == t {
            o
        } else if b == o {
            t
        } else {
            out.text_conflicts.push(path.clone());
            o
        };
        if let Some(bytes) = take {
            out.files.files.insert(path.clone(), bytes.clone());
        }
    }
    Ok(out)
}

/// The merge of [`merge`] with every conflict decided by `choices`. Fails with
/// [`FormatError::Invalid`] if a conflict is left without a choice.
///
/// Interface state (wave 12): file choices only; strand B adds field choices.
pub fn resolve(
    base: &FileSet,
    ours: &FileSet,
    theirs: &FileSet,
    choices: &[ConflictChoice],
) -> Result<FileSet, FormatError> {
    let mut result = merge(base, ours, theirs)?;
    if let Some(c) = result.conflicts.first() {
        return Err(FormatError::Invalid {
            path: c.file.clone(),
            message: "field conflicts cannot be resolved yet".into(),
        });
    }
    for path in &result.text_conflicts {
        let side = choices.iter().find_map(|c| match c {
            ConflictChoice::File { path: p, side } if p == path => Some(*side),
            _ => None,
        });
        let from = match side {
            Some(Side::Ours) => ours,
            Some(Side::Theirs) => theirs,
            None => {
                return Err(FormatError::Invalid {
                    path: path.clone(),
                    message: "the conflict has no resolution".into(),
                });
            }
        };
        match from.files.get(path) {
            Some(bytes) => result.files.files.insert(path.clone(), bytes.clone()),
            None => result.files.files.remove(path),
        };
    }
    Ok(result.files)
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
