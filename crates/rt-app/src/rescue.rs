// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret rescue (wave 12): the secret scanner over the whole history, and rewriting the
//! history so a found value is replaced by a `{{variable}}` in every commit.
//!
//! - The scan covers every commit reachable from `HEAD` and the local branches, each blob
//!   once. A finding is known by [`HistoryFinding::id`] (where it was first seen); its value
//!   never leaves the shell: the rewrite reads it again from that blob.
//! - The rewrite runs only after the user confirmed it, with no unsaved changes and a clean
//!   collection; first it saves every rewritten branch under [`BACKUP_PREFIX`] (not a branch,
//!   so a later scan of the branches is clean; it still holds the secret until deleted). It
//!   rewrites the local branches that contain a finding, puts the value into the secret store
//!   (environment and variable of the plan) and updates the work tree to the new `HEAD`.
//!   Remote-tracking refs and tags stay; the result lists the branches that need a force push.
//!   The app warns before the force push and tells the user to rotate the secret.
//! - Natively with gitoxide (`rt-git`), in the web app with isomorphic-git; the text
//!   replacement is shared ([`replace_value`]).
//!
//! Owner: wave 12 interface; implementation strand G.

use serde::{Deserialize, Serialize};

use crate::git::GitCommit;
use crate::secret_scan::SecretFinding;

/// Backups of rewritten branches: `refs/requesttailor/backup/<unix seconds>/<branch>`.
pub const BACKUP_PREFIX: &str = "refs/requesttailor/backup/";

/// A credential found somewhere in the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryFinding {
    /// `<commit oid>:<path>:<line>` of the first (oldest) occurrence.
    pub id: String,
    /// As the scanner reports it (snippet masked), at the first occurrence.
    pub finding: SecretFinding,
    /// The commit that introduced it.
    pub first: GitCommit,
    /// Commits that contain the value, oldest first.
    pub commits: Vec<String>,
    /// Local branches that contain it.
    pub branches: Vec<String>,
    /// `HEAD`'s files still contain it (a commit alone does not remove it from history).
    pub in_head: bool,
}

/// Progress of a history scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scanned: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HistoryScan {
    /// Commits scanned.
    pub commits: u32,
    pub findings: Vec<HistoryFinding>,
}

/// What to rewrite (`RescueApi.rewrite`), after the user confirmed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RescuePlan {
    /// [`HistoryFinding::id`]s; all must be about the same value.
    pub findings: Vec<String>,
    /// The variable the value becomes, e.g. `apiToken` → `{{apiToken}}`.
    pub variable: String,
    /// The environment whose secret store entry gets the value.
    pub environment: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewrittenBranch {
    pub name: String,
    pub old: String,
    pub new: String,
    /// Has an upstream: needs a force push to remove the value there.
    pub pushed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RescueResult {
    /// Commits written anew.
    pub rewritten: u32,
    pub branches: Vec<RewrittenBranch>,
    /// Ref prefix of this rescue's backups, e.g. `refs/requesttailor/backup/1795000000/`.
    pub backup: String,
}

/// `text` with every occurrence of `value` replaced by `{{variable}}`.
///
/// Interface state (wave 12): the plain replacement; strand G refines it (values inside
/// quoted YAML scalars, base64 forms, …).
pub fn replace_value(text: &str, value: &str, variable: &str) -> String {
    if value.is_empty() {
        return text.to_owned();
    }
    text.replace(value, &format!("{{{{{variable}}}}}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_value_becomes_a_variable() {
        assert_eq!(
            super::replace_value("token: abc123\nx: abc123\n", "abc123", "apiToken"),
            "token: {{apiToken}}\nx: {{apiToken}}\n"
        );
        assert_eq!(super::replace_value("a", "", "v"), "a");
    }
}
