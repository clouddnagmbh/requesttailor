// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic merge for the app (wave 12): the field conflicts of a merge in progress and their
//! resolution, on top of [`rt_format::merge`].
//!
//! Conflicts are recomputed, never stored: a stopped merge leaves base, ours and theirs in
//! Git's index (stages 1–3, `GitApi.conflict`), and the shells build the three
//! [`rt_format::FileSet`]s from them (the conflicted files plus the rest of the collection at
//! `HEAD`, so folders and the collection file are known).
//!
//! Owner: wave 12 interface; implementation strand B.

use serde::{Deserialize, Serialize};

pub use rt_format::merge::{ConflictChoice, FieldConflict, MergeResult, Side};

/// The conflicts of the merge in progress (`MergeApi.conflicts`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MergeState {
    /// A merge is in progress (`MERGE_HEAD`).
    pub merging: bool,
    /// Conflicts per field, to resolve in the app.
    pub conflicts: Vec<FieldConflict>,
    /// Files that need a decision as a whole (`ConflictChoice::File`).
    pub text_conflicts: Vec<String>,
}

impl From<&MergeResult> for MergeState {
    fn from(r: &MergeResult) -> Self {
        Self {
            merging: !r.is_clean(),
            conflicts: r.conflicts.clone(),
            text_conflicts: r.text_conflicts.clone(),
        }
    }
}
