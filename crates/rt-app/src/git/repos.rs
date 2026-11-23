// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Repositories in the collection set (wave 15, strand A): any Git repository can be opened,
//! not only one that holds a collection.
//!
//! Files stay the truth: no file lists the repositories. The shells derive them from the roots
//! of the collection set ([`crate::collections::CollectionSet`]): every repository that holds a
//! collection of the set, and every root that is (or lies inside) a Git repository without any
//! collection. Such a *repository root* has no collection and is not scaffolded as a new one
//! (a root without a collection and outside Git still opens as a new collection, as before).
//!
//! One repository is *active*: the one the user chose last in the sidebar, else the one of the
//! active collection. Every `GitApi` command acts on the active repository at the time of the
//! call (wave 14: "the repository of the active collection"; the same thing while a collection
//! is active). Paths are repository-relative with a leading `/` in everything wave 15 adds.

use serde::{Deserialize, Serialize};

/// A repository of the set, for the sidebar and the Git views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryEntry {
    /// Stable on this device: the same string as [`CollectionEntry::repo`] of its collections
    /// (desktop: canonical path of the working tree; web: the root's id plus the folder).
    ///
    /// [`CollectionEntry::repo`]: crate::collections::CollectionEntry::repo
    pub id: String,
    /// Folder name of the working tree.
    pub name: String,
    /// For display only (path on desktop, directory names in the browser).
    pub location: String,
    /// The root ([`CollectionRoot::id`](crate::collections::CollectionRoot::id)) it was found
    /// through; a repository reached from several roots names the first.
    pub root: String,
    /// Ids of the set's collections inside it, in sidebar order (empty for a repository root
    /// without collections).
    #[serde(default)]
    pub collections: Vec<String>,
    /// The working tree is a linked worktree (`.git` is a file) or a submodule's checkout.
    #[serde(default)]
    pub linked: bool,
}

/// What the sidebar shows next to a repository, asked for per repository (cheap: branch,
/// ahead/behind against the upstream, the number of changed files).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRepoSummary {
    /// [`RepositoryEntry::id`].
    pub id: String,
    /// `None`: detached `HEAD` or no commit yet.
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Changed, new and conflicted files of the whole working tree.
    pub changes: u32,
    /// A merge, cherry-pick, revert or rebase is half done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<GitOperation>,
}

/// A Git operation that stopped half way (conflicts, or started on the command line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitOperation {
    Merge,
    CherryPick,
    Revert,
    /// A rebase started outside the app (`.git/rebase-merge`, `.git/rebase-apply`); the app
    /// offers only to abort it or to finish it on the command line.
    Rebase,
}

/// Clone any repository into a new folder and add it to the set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCloneRequest {
    /// HTTPS URL, or (desktop) a local path / `file://` URL, as `GitApi.clone` takes it.
    pub url: String,
    /// Folder to clone into (a new folder `name` is created inside it); `None`: the shell
    /// asks with its folder picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Name of the new folder; `None`: from the URL (`orders.git` → `orders`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Branch to check out; `None`: the remote's default branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// The default folder name for a clone of `url`: its last path segment without `.git`
/// (`https://host/acme/orders.git` → `orders`, `/repos/api/` → `api`), `repository` if none.
pub fn clone_folder_name(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\', ':']).next().unwrap_or_default();
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() || name == "." || name == ".." {
        "repository".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_folder_names_come_from_the_url() {
        assert_eq!(
            clone_folder_name("https://github.com/acme/orders.git"),
            "orders"
        );
        assert_eq!(clone_folder_name("https://gitlab.com/g/sub/api"), "api");
        assert_eq!(clone_folder_name("git@github.com:acme/web.git"), "web");
        assert_eq!(clone_folder_name("/repos/api/"), "api");
        assert_eq!(clone_folder_name("file:///tmp/x.git"), "x");
        assert_eq!(clone_folder_name(""), "repository");
        assert_eq!(clone_folder_name("https://host/.git"), "repository");
    }

    #[test]
    fn operations_are_kebab_case() {
        assert_eq!(
            serde_json::to_value(GitOperation::CherryPick).unwrap(),
            "cherry-pick"
        );
    }
}
