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

/// Generic parts in the Tailor Kit (`tk_git_types::repos`).
pub use tk_git_types::repos::*;

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

/// What a root the user opened or added is (the one rule for every shell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// It holds collections (itself or in subfolders).
    Collections,
    /// No collection, but in a Git repository: a *repository root*, never scaffolded.
    Repository,
    /// No collection and outside Git: opens as a new collection, as before wave 15.
    NewCollection,
}

/// Classifies a root: `has_collections` (discovery found any), `repo` (the Git repository it
/// is in, if any).
pub fn root_kind(has_collections: bool, repo: Option<&str>) -> RootKind {
    match (has_collections, repo) {
        (true, _) => RootKind::Collections,
        (false, Some(_)) => RootKind::Repository,
        (false, None) => RootKind::NewCollection,
    }
}

/// A root that is a repository root: its id, and the repository it is in (the id every
/// [`RepositoryEntry`] has).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRoot {
    /// [`crate::collections::CollectionRoot::id`].
    pub root: String,
    /// [`RepositoryEntry::id`].
    pub repo: String,
}

/// The repositories of a set in sidebar order: first those that hold collections (in the order
/// of the collections), then the repository roots without one. A repository reached through
/// several roots is listed once, under the first. `describe` gives name, location and whether
/// it is a linked worktree for a repository id (shells know their paths).
pub fn repositories(
    entries: &[crate::collections::CollectionEntry],
    repo_roots: &[RepoRoot],
    describe: impl Fn(&str) -> (String, String, bool),
) -> Vec<RepositoryEntry> {
    let mut out: Vec<RepositoryEntry> = Vec::new();
    let mut add = |id: &str, root: &str, collection: Option<&str>| {
        if let Some(r) = out.iter_mut().find(|r| r.id == id) {
            if let Some(c) = collection {
                r.collections.push(c.to_owned());
            }
            return;
        }
        let (name, location, linked) = describe(id);
        out.push(RepositoryEntry {
            id: id.to_owned(),
            name,
            location,
            root: root.to_owned(),
            collections: collection.map(str::to_owned).into_iter().collect(),
            linked,
        });
    };
    for e in entries {
        if let Some(repo) = &e.repo {
            add(repo, &e.root, Some(&e.id));
        }
    }
    for r in repo_roots {
        add(&r.repo, &r.root, None);
    }
    out
}

/// The repository Git acts on: the one `chosen` in the sidebar if it is still in the set, else
/// the one of the active collection (`None` outside Git or without a collection).
pub fn active_repository(
    repositories: &[RepositoryEntry],
    entries: &[crate::collections::CollectionEntry],
    chosen: Option<&str>,
    active_collection: Option<&str>,
) -> Option<String> {
    if let Some(id) = chosen.filter(|id| repositories.iter().any(|r| r.id == *id)) {
        return Some(id.to_owned());
    }
    let active = active_collection?;
    entries
        .iter()
        .find(|e| e.id == active)
        .and_then(|e| e.repo.clone())
}
