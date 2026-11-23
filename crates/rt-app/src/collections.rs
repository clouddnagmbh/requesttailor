// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collections in the sidebar (wave 14): every collection of the folders the user opened or
//! added is a top-level node, like a list of collections in other API clients.
//!
//! Files stay the truth: there is no file that lists the collections. A *collection set* is what
//! the shells derive from the *roots* (folders the user opened or added, remembered per device
//! like recent collections): a root that is a collection gives one collection; a root that
//! holds collections in subfolders gives each of them ([`discover`], depth [`MAX_DEPTH`],
//! skipping hidden folders, `node_modules` and `target`). A root without any collection opens
//! as a new collection, as before wave 14.
//!
//! The name is "collection set", never "workspace": `rt_app::Workspace` is the in-memory state
//! of one open collection, rt-wasm exports it as the `Workspace` class, and distributions use the
//! word for shared team spaces.
//!
//! One collection of the set is *active*: the one of the active tab (or the node the user chose
//! last). Every per-collection command (tree, requests, send, environments, secrets, Git,
//! cookies, scripts, plugin tabs, runner, ...) acts on the active collection at the time of the
//! call; long-running calls (runner, streams, compare, bisect, MCP tests) keep that collection
//! until they end. What the sidebar shows for all collections at once (trees, unsaved changes,
//! save, search) names the collection explicitly ([`RequestRef`], [`CollectionSearchHit`],
//! [`CollectionChanges`]).
//!
//! Owner in wave 14: strand A (shells, set management, [`scaffold`]). [`enter`] and
//! [`discover`] are the one rule for every shell.

use serde::{Deserialize, Serialize};

use crate::{AppError, FileChange, SearchHit};

/// How deep discovery looks below a root (the root itself is depth 0).
pub const MAX_DEPTH: usize = 4;

/// At most this many collections per root; the rest is left out ([`Discovery::truncated`]).
pub const MAX_COLLECTIONS: usize = 64;

/// Whether discovery enters the directory `name` at `depth` (its depth below the root, the
/// root's children are depth 1). Hidden directories (`.git`, `.github`, ...), `node_modules`
/// and `target` are never entered, nor anything deeper than [`MAX_DEPTH`].
pub fn enter(name: &str, depth: usize) -> bool {
    depth <= MAX_DEPTH && !name.starts_with('.') && name != "node_modules" && name != "target"
}

/// The collections a root holds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Discovery {
    /// Directories relative to the root, `/`-separated, `""` for the root itself; the root
    /// first, then sorted by path. Nested collections are separate entries (as
    /// [`rt_format::FileSet::from_dir`] leaves them out of the outer one).
    pub dirs: Vec<String>,
    /// More than [`MAX_COLLECTIONS`] were found.
    pub truncated: bool,
}

/// The collections below a root, from the directories (relative, `/`-separated, `""` = root)
/// in which a walk with [`enter`] found an `opencollection.yml`. No directory: the root is a
/// collection of its own (a new one if the folder is empty).
pub fn discover<I, S>(manifest_dirs: I) -> Discovery
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut dirs: Vec<String> = manifest_dirs
        .into_iter()
        .map(|d| d.as_ref().trim_matches('/').to_owned())
        .filter(|d| {
            d.split('/')
                .enumerate()
                .all(|(i, part)| d.is_empty() || enter(part, i + 1))
        })
        .collect();
    dirs.sort_by(|a, b| (!a.is_empty(), a).cmp(&(!b.is_empty(), b)));
    dirs.dedup();
    if dirs.is_empty() {
        dirs.push(String::new());
    }
    let truncated = dirs.len() > MAX_COLLECTIONS;
    dirs.truncate(MAX_COLLECTIONS);
    Discovery { dirs, truncated }
}

/// One collection in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionEntry {
    /// `CollectionInfo.id`: stable on this device (desktop: canonical path of the collection
    /// folder, the same as when it is opened on its own; web: the root's id plus `dir`). Also
    /// `SecretKey::collection`, so secrets, tokens, cookies and trust stay per collection.
    pub id: String,
    /// `name` of `opencollection.yml`, else the folder name.
    pub name: String,
    /// For display only (path on desktop, directory names in the browser).
    pub location: String,
    /// The root ([`CollectionRoot::id`]) it was found in.
    pub root: String,
    /// Its directory relative to the root, `""` if the root is the collection.
    pub dir: String,
    /// The Git repository that holds it (repository root, as an id); collections with the
    /// same `repo` share one Git panel. `None` outside Git.
    pub repo: Option<String>,
    /// Number of unsaved changes (requests, environments, companion files).
    pub dirty: usize,
}

/// A folder the user opened or added ("Add collection…"), remembered per device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionRoot {
    pub id: String,
    pub name: String,
    pub location: String,
    /// Ids of the collections found in it, in sidebar order.
    pub collections: Vec<String>,
    /// More collections than [`MAX_COLLECTIONS`] were found; the rest is not shown.
    pub truncated: bool,
}

/// What the sidebar shows: roots in the order opened and added, their collections, and the
/// active one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CollectionSet {
    pub roots: Vec<CollectionRoot>,
    /// All collections, in sidebar order (roots in order, then each root's collections).
    pub collections: Vec<CollectionEntry>,
    /// Id of the active collection.
    pub active: Option<String>,
}

/// A request in one of the set's collections: tabs, search hits, history entries and runner
/// targets name it like this.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequestRef {
    /// [`CollectionEntry::id`].
    pub collection: String,
    /// Request path relative to that collection's root.
    pub path: String,
}

/// A search hit across the set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionSearchHit {
    pub collection: String,
    #[serde(flatten)]
    pub hit: SearchHit,
}

/// What saving wrote, per collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionChanges {
    pub collection: String,
    pub changes: Vec<FileChange>,
}

/// A new collection from the "New…" dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewCollection {
    /// `name` in `opencollection.yml`.
    pub name: String,
    /// The root to create it in (`CollectionRoot::id`); `None`: the shell asks for a folder
    /// (folder picker) and adds it as a new root.
    pub root: Option<String>,
    /// Folder for it inside the root, relative, `/`-separated (default in the dialog: the
    /// name as a folder name). Must not exist yet or be empty.
    pub dir: String,
    /// Optional `baseUrl` variable: of the first environment if one is named, else of the
    /// collection.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Optional first environment.
    #[serde(default)]
    pub environment: Option<String>,
}

/// The files of a new collection, relative to its folder, written through `rt-format` (only
/// `rt-format` knows OpenCollection). Strand A implements it.
pub fn scaffold(spec: &NewCollection) -> Result<Vec<FileChange>, AppError> {
    let _ = spec;
    Err(AppError::Unsupported(
        "new collections arrive in wave 14 (strand A)".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_skips_hidden_dependencies_and_deep_folders() {
        assert!(enter("api", 1));
        assert!(!enter(".git", 1));
        assert!(!enter("node_modules", 2));
        assert!(!enter("target", 1));
        assert!(enter("d", MAX_DEPTH));
        assert!(!enter("e", MAX_DEPTH + 1));
        let found = discover([
            "services/orders",
            "",
            "apps/web/node_modules/x",
            "a/b/c/d/e",
            "billing/",
            "billing",
        ]);
        assert_eq!(found.dirs, vec!["", "billing", "services/orders"]);
        assert!(!found.truncated);
    }

    #[test]
    fn a_folder_without_collections_is_one_new_collection() {
        let found = discover(Vec::<String>::new());
        assert_eq!(found.dirs, vec![""]);
    }

    #[test]
    fn discovery_stops_at_the_limit() {
        let dirs: Vec<String> = (0..MAX_COLLECTIONS + 3)
            .map(|i| format!("c{i:03}"))
            .collect();
        let found = discover(&dirs);
        assert_eq!(found.dirs.len(), MAX_COLLECTIONS);
        assert!(found.truncated);
    }

    #[test]
    fn search_hits_name_their_collection_flat() {
        let hit = CollectionSearchHit {
            collection: "/c".into(),
            hit: SearchHit {
                path: "a.yml".into(),
                name: "A".into(),
                method: "GET".into(),
                field: crate::SearchField::Name,
                snippet: "A".into(),
                ranges: vec![(0, 1)],
            },
        };
        let json = serde_json::to_value(&hit).unwrap();
        assert_eq!(json["collection"], "/c");
        assert_eq!(json["path"], "a.yml");
        assert_eq!(
            serde_json::from_value::<CollectionSearchHit>(json).unwrap(),
            hit
        );
    }
}
