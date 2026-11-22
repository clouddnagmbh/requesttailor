// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic diffs for the app (wave 12): [`rt_format::diff`] plus what only the app knows.
//! Every shell (desktop, web through rt-wasm, `rt diff`) goes through [`diff`], so masking
//! happens in one place: besides secret variables (masked by `rt-format`), values the secret
//! scanner flags (`secret_scan`) never appear in clear.
//!
//! Owner: wave 12 interface; implementation strand A.

use serde::{Deserialize, Serialize};

pub use rt_format::diff::{
    ChangeKind, ChangeTarget, ChangedFile, CollectionDiff, DiffValue, FieldChange, FieldPath,
    FileStatus, ModelChange,
};

use crate::AppError;

/// What the Git view diffs (`DiffApi`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DiffScope {
    /// The saved files against `HEAD`.
    Working,
    /// A commit against its first parent (the empty tree for a root commit).
    Commit { rev: String },
    /// `from` → `to`, e.g. two branches.
    Between { from: String, to: String },
}

/// The semantic diff of `old` → `new`, masked.
///
/// Interface state (wave 12): [`rt_format::diff::diff`] unchanged; strand A adds the scanner
/// masking.
pub fn diff(
    old: &rt_format::FileSet,
    new: &rt_format::FileSet,
) -> Result<CollectionDiff, AppError> {
    Ok(rt_format::diff::diff(old, new)?)
}
