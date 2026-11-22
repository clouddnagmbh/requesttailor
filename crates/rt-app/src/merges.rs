// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic merge for the app (wave 12): the field conflicts of a merge in progress and their
//! resolution, on top of [`rt_format::merge`].
//!
//! Conflicts are recomputed, never stored: a stopped merge leaves base, ours and theirs in
//! Git's index (stages 1–3, `GitApi.conflict`), and the shells build the three
//! [`rt_format::FileSet`]s from them (the conflicted files plus the rest of the collection at
//! `HEAD`, so folders and the collection file are known). [`state`] and [`resolution`] are
//! the shared logic: the desktop app calls them through `rt-git`, the web app through
//! `rt-wasm` (`merges_js.rs`), so a merge started on the command line (`git merge`, with or
//! without `rt merge-driver`) shows the same conflicts everywhere.
//!
//! Owner: wave 12 interface; implementation strand B.

use serde::{Deserialize, Serialize};

/// The files the merge works on (re-exported for the shells).
pub use rt_format::FileSet;
pub use rt_format::merge::{ConflictChoice, FieldConflict, MergeResult, Side};

use crate::AppError;

/// The conflicts of the merge in progress (`MergeApi.conflicts`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MergeState {
    /// A merge is in progress (`MERGE_HEAD`).
    pub merging: bool,
    /// Conflicts per field, to resolve in the app.
    pub conflicts: Vec<FieldConflict>,
    /// Files that need a decision as a whole (`ConflictChoice::File`).
    pub text_conflicts: Vec<String>,
    /// Files Git left conflicted that merge without a conflict on the model (e.g. after a
    /// `git merge` without `rt merge-driver`): `MergeApi.resolve` writes them too. Added by
    /// strand B.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clean: Vec<String>,
}

impl From<&MergeResult> for MergeState {
    fn from(r: &MergeResult) -> Self {
        Self {
            merging: !r.is_clean(),
            conflicts: r.conflicts.clone(),
            text_conflicts: r.text_conflicts.clone(),
            clean: Vec::new(),
        }
    }
}

/// One conflicted file of Git's index: its stages as text (`None`: that side has no such
/// file), the path relative to the collection root.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ConflictStages {
    pub path: String,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

/// Base, ours and theirs of the merge in progress: `context` (the collection's files at
/// `HEAD`) with each conflicted file replaced by its stage.
pub fn sides(context: &FileSet, stages: &[ConflictStages]) -> [FileSet; 3] {
    let mut out = [context.clone(), context.clone(), context.clone()];
    for s in stages {
        for (set, side) in out.iter_mut().zip([&s.base, &s.ours, &s.theirs]) {
            match side {
                Some(text) => set.files.insert(s.path.clone(), text.clone().into_bytes()),
                None => set.files.remove(&s.path),
            };
        }
    }
    out
}

fn merged(
    context: &FileSet,
    stages: &[ConflictStages],
    choices: &[ConflictChoice],
) -> Result<MergeResult, AppError> {
    let [base, ours, theirs] = sides(context, stages);
    Ok(rt_format::merge::merge_with(
        &base, &ours, &theirs, choices,
    )?)
}

/// The conflicts of the merge in progress, recomputed from the index stages.
pub fn state(
    context: &FileSet,
    stages: &[ConflictStages],
    merging: bool,
) -> Result<MergeState, AppError> {
    let r = merged(context, stages, &[])?;
    let conflicted = |p: &str| {
        r.conflicts.iter().any(|c| c.file == p) || r.text_conflicts.iter().any(|t| t == p)
    };
    Ok(MergeState {
        merging,
        clean: stages
            .iter()
            .map(|s| s.path.clone())
            .filter(|p| !conflicted(p))
            .collect(),
        conflicts: r.conflicts,
        text_conflicts: r.text_conflicts,
    })
}

/// A file to write when resolving: its path and content (`None`: delete it).
pub type Resolved = (String, Option<Vec<u8>>);

/// What `MergeApi.resolve(choices)` writes: each conflicted file whose conflicts `choices`
/// all decide (or that merges cleanly), with its content (`None`: delete it). Files with a
/// conflict left stay as they are.
pub fn resolution(
    context: &FileSet,
    stages: &[ConflictStages],
    choices: &[ConflictChoice],
) -> Result<Vec<Resolved>, AppError> {
    let r = merged(context, stages, choices)?;
    Ok(stages
        .iter()
        .filter(|s| {
            !r.conflicts.iter().any(|c| c.file == s.path) && !r.text_conflicts.contains(&s.path)
        })
        .map(|s| (s.path.clone(), r.files.files.get(&s.path).cloned()))
        .collect())
}

/// The merge of a pull (`base`, `ours`, `theirs`: the collection's files at the merge base,
/// `HEAD` and the upstream), with what to put in the working tree for each conflicted file
/// (see [`rt_format::merge::merge_marked`]).
pub fn pull_merge(
    base: &FileSet,
    ours: &FileSet,
    theirs: &FileSet,
) -> Result<(MergeResult, std::collections::BTreeMap<String, Vec<u8>>), AppError> {
    Ok(rt_format::merge::merge_marked(base, ours, theirs, &[])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_format::diff::ChangeTarget;

    const COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: M\n";

    fn request(url: &str) -> String {
        format!("info:\n  name: Get\n  type: http\n\nhttp:\n  method: GET\n  url: {url}\n")
    }

    fn context() -> FileSet {
        FileSet {
            files: [
                (
                    "opencollection.yml".to_string(),
                    COLLECTION.as_bytes().to_vec(),
                ),
                ("get.yml".to_string(), request("https://ours").into_bytes()),
            ]
            .into(),
        }
    }

    fn stages(theirs: &str) -> Vec<ConflictStages> {
        vec![ConflictStages {
            path: "get.yml".into(),
            base: Some(request("https://base")),
            ours: Some(request("https://ours")),
            theirs: Some(request(theirs)),
        }]
    }

    #[test]
    fn conflicts_come_from_the_stages_and_resolve_per_field() {
        let s = state(&context(), &stages("https://theirs"), true).unwrap();
        assert!(s.merging);
        assert_eq!(s.conflicts.len(), 1);
        assert!(s.clean.is_empty());
        let none = resolution(&context(), &stages("https://theirs"), &[]).unwrap();
        assert!(none.is_empty());
        let choice = ConflictChoice::Field {
            target: ChangeTarget::Request {
                path: "get.yml".into(),
            },
            field: vec!["url".into()],
            side: Side::Theirs,
        };
        let files = resolution(&context(), &stages("https://theirs"), &[choice]).unwrap();
        assert_eq!(
            files,
            vec![(
                "get.yml".to_string(),
                Some(request("https://theirs").into_bytes())
            )]
        );
    }

    #[test]
    fn a_file_that_merges_on_the_model_is_clean() {
        let s = state(&context(), &stages("https://base"), true).unwrap();
        assert!(s.conflicts.is_empty() && s.text_conflicts.is_empty());
        assert_eq!(s.clean, vec!["get.yml".to_string()]);
    }
}
