// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The commit graph of a whole repository (wave 14, strand F): every local and remote branch,
//! tag and `HEAD`, with the commits behind them, paged for the lanes view in the main area.
//!
//! Generic on purpose: nothing here assumes the repository is a collection (wave 15 opens any
//! repository). Paths are repository-relative with a leading `/` (the wave-12 convention for
//! "relative to the repository root"). A changed file that is a request of a collection in the
//! set names that collection ([`GitChangedFile::collection`]), so the UI shows the semantic
//! diff for it and the line diff for every other file.
//!
//! The lane layout is computed once in the UI (`apps/ui/src/git/graph/layout.ts`), the same for
//! every shell; the shells only list commits and refs (natively `rt-git/src/graph.rs`, in the
//! browser `apps/web/src/git/graph.ts`). Additions are additive (new fields default).

use serde::{Deserialize, Serialize};

use super::{GitCommit, GitFileChange};

/// What a ref is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitRefKind {
    /// `refs/heads/*`.
    Local,
    /// `refs/remotes/*` (`origin/HEAD` is left out).
    Remote,
    /// `refs/tags/*` (annotated tags peeled to their commit).
    Tag,
    /// `HEAD` while detached.
    Head,
}

/// A branch, remote branch or tag, with what the graph and the wizards need to explain it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRef {
    /// Full name (`refs/heads/main`, `refs/remotes/origin/main`, `refs/tags/v1`, `HEAD`).
    pub full: String,
    /// Short name as shown (`main`, `origin/main`, `v1`, `HEAD`).
    pub name: String,
    pub kind: GitRefKind,
    /// The commit it points to (full id).
    pub target: String,
    /// The checked-out branch (or the detached `HEAD`).
    #[serde(default)]
    pub current: bool,
    /// Local branches: the upstream (`origin/main`), if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    /// Local branches with an upstream: commits only here / only there.
    #[serde(default)]
    pub ahead: u32,
    #[serde(default)]
    pub behind: u32,
    /// Annotated tags: the message's first line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// One page of the graph.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GitGraphQuery {
    /// Commits to skip (the order is stable while no ref moves).
    #[serde(default)]
    pub skip: u32,
    /// At most this many commits (the UI asks for pages of a few thousand).
    pub limit: u32,
    /// Start from these refs (full names); `None`: all refs and `HEAD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<String>>,
}

/// A commit in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitGraphCommit {
    #[serde(flatten)]
    pub commit: GitCommit,
    /// Parent ids, first parent first (two or more: a merge commit).
    pub parents: Vec<String>,
    /// Author email, for avatars and filters.
    #[serde(default)]
    pub email: String,
    /// Full names of the refs pointing at it ([`GitRef::full`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

/// Commits in topological order, newest first (a child always before its parents, ties by
/// commit time), and all refs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GitGraphPage {
    pub commits: Vec<GitGraphCommit>,
    /// All refs, on every page (cheap; the graph labels and the branch list use them).
    pub refs: Vec<GitRef>,
    /// The commit `HEAD` points to; `None` in a repository without commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// `HEAD` is not on a branch.
    #[serde(default)]
    pub detached: bool,
    /// More commits follow (ask with `skip + commits.len()`).
    #[serde(default)]
    pub more: bool,
}

/// A file a commit (or a range) changed, for the commit details next to the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChangedFile {
    /// Repository-relative, with a leading `/`.
    pub path: String,
    pub change: GitFileChange,
    /// Renames: the old path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Binary content: no line diff, metadata only.
    #[serde(default)]
    pub binary: bool,
    /// The collection of the set this file is a request (or environment, folder, collection
    /// file) of: `CollectionEntry::id`. `None`: any other file, shown as a line diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
}
