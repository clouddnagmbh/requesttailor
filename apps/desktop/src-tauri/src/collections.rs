// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 14, strand A: the collections in the sidebar on the desktop. The set itself lives in
//! [`Core`](crate::core::Core); this module finds the collections of a folder on disk (the one
//! rule of `rt_app::collections::{enter, discover}`), remembers the roots in the data folder,
//! and has the `collections_*` commands (`CollectionSetApi` in `apps/ui`).
//!
//! - Discovery never follows symbolic links and never enters what `enter` refuses (hidden
//!   folders, `node_modules`, `target`, deeper than `MAX_DEPTH`); it stops after
//!   `MAX_COLLECTIONS` (`truncated`). Only `opencollection.yml` of each collection is read
//!   (for its name); the rest is read when the collection is.
//! - The roots and the active collection are kept in `collection-roots.json` in the data
//!   folder (in measurement mode in `RT_PERF_DIR`, like the recent list). No file in a
//!   collection or repository lists collections.

use std::io;
use std::path::{Path, PathBuf};

use rt_app::collections::{
    CollectionChanges, CollectionSearchHit, CollectionSet, MAX_COLLECTIONS, NewCollection,
    discover, enter,
};
use rt_app::{AppError, TreeNode};
use rt_format::{COLLECTION_FILE, FileSet};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::commands::{AppCore, CommandError};
use crate::core::CollectionInfo;
use crate::files;

/// File name of the remembered roots in the data folder.
pub const ROOTS_FILE: &str = "collection-roots.json";

/// One collection found below a root.
#[derive(Debug, Clone)]
pub struct FoundCollection {
    /// Canonical path, the collection's id.
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    /// Relative to the root, `/`-separated, `""` for the root itself.
    pub dir: String,
    pub repo: Option<String>,
}

/// What a root holds.
#[derive(Debug, Clone)]
pub struct Found {
    /// Canonical path of the root.
    pub id: String,
    pub path: PathBuf,
    /// Folder name.
    pub name: String,
    /// In sidebar order; empty if the folder holds no collection.
    pub collections: Vec<FoundCollection>,
    pub truncated: bool,
    /// Wave 15: the Git repository the root is in (its working-tree root, as an id) if it holds
    /// no collection: a *repository root*. `None` for a root with collections, and for a
    /// folder outside Git.
    pub repo_root: Option<String>,
}

/// The collections in the folder `root` (see the module documentation).
pub fn find(root: &Path) -> io::Result<Found> {
    let (path, id) = files::collection_id(root)?;
    if !path.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotADirectory, "not a folder"));
    }
    let mut manifests = Vec::new();
    if path.join(COLLECTION_FILE).is_file() {
        manifests.push(String::new());
    }
    walk(&path, "", 1, &mut manifests)?;
    let found = !manifests.is_empty();
    let discovery = discover(&manifests);
    let repo_of_root = repo_of(&path);
    let collections = if found {
        discovery
            .dirs
            .iter()
            .map(|dir| {
                let cpath = if dir.is_empty() {
                    path.clone()
                } else {
                    path.join(dir)
                };
                FoundCollection {
                    id: cpath.to_string_lossy().into_owned(),
                    name: manifest_name(&cpath).unwrap_or_else(|| folder_name(&cpath)),
                    // Inside a repository the root is part of, the same repository (no walk
                    // up per collection); a collection may still be a repository of its own.
                    repo: own_repo(&cpath).or_else(|| repo_of_root.clone()),
                    path: cpath,
                    dir: dir.clone(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let repo_root = match rt_app::git::repos::root_kind(found, repo_of_root.as_deref()) {
        rt_app::git::repos::RootKind::Repository => repo_of_root,
        _ => None,
    };
    Ok(Found {
        name: folder_name(&path),
        id,
        path,
        collections,
        truncated: discovery.truncated,
        repo_root,
    })
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) -> io::Result<()> {
    // One more than the limit is enough to know that the list is cut.
    if out.len() > MAX_COLLECTIONS {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // `file_type` does not follow symbolic links: links are skipped.
        if !kind.is_dir() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !enter(&name, depth) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.path().join(COLLECTION_FILE).is_file() {
            out.push(rel.clone());
        }
        // A folder that cannot be read (permissions) is left out, not an error for the root.
        let _ = walk(&entry.path(), &rel, depth + 1, out);
    }
    Ok(())
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// `info.name` of the collection's `opencollection.yml` (only that file is read).
fn manifest_name(dir: &Path) -> Option<String> {
    let bytes = std::fs::read(dir.join(COLLECTION_FILE)).ok()?;
    let mut set = FileSet::default();
    set.files.insert(COLLECTION_FILE.to_owned(), bytes);
    let doc = rt_format::read_collection(&set).ok()?;
    Some(doc.model().name.trim().to_owned()).filter(|n| !n.is_empty())
}

fn own_repo(dir: &Path) -> Option<String> {
    dir.join(".git")
        .exists()
        .then(|| dir.to_string_lossy().into_owned())
}

/// The Git repository that holds `path` (its working-tree root, as an id), found by looking
/// for `.git` in `path` and its parents. For a folder that does not exist yet, its parents.
pub fn repo_of(path: &Path) -> Option<String> {
    path.ancestors().find_map(own_repo)
}

/// Whether a new collection may go to `path`: it does not exist, or is an empty folder.
pub fn is_free(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) => e.kind() == io::ErrorKind::NotFound && !path.exists(),
    }
}

// ---- remembered roots ----

/// What is kept in [`ROOTS_FILE`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRoots {
    /// Canonical root paths, in sidebar order.
    pub roots: Vec<String>,
    pub active: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct StoredFile {
    version: u32,
    #[serde(flatten)]
    saved: SavedRoots,
}

/// Where the roots are kept: the measurement folder (`RT_PERF_DIR`) in measurement mode, else
/// the data folder.
fn roots_file(data_dir: &Path) -> PathBuf {
    crate::recent::perf::dir()
        .unwrap_or_else(|| data_dir.to_path_buf())
        .join(ROOTS_FILE)
}

/// The remembered roots; `None` if there are none or the file cannot be read.
pub fn load_roots(data_dir: &Path) -> Option<SavedRoots> {
    let text = std::fs::read_to_string(roots_file(data_dir)).ok()?;
    let file: StoredFile = serde_json::from_str(&text).ok()?;
    (file.version == 1 && !file.saved.roots.is_empty()).then_some(file.saved)
}

/// Remembers the roots of `set` and its active collection; failing to write never fails a
/// command (the sidebar still works, it is only not restored at the next start).
pub fn save_roots(data_dir: &Path, set: &CollectionSet) {
    let file = roots_file(data_dir);
    let stored = StoredFile {
        version: 1,
        saved: SavedRoots {
            roots: set.roots.iter().map(|r| r.id.clone()).collect(),
            active: set.active.clone(),
        },
    };
    let written = (|| -> io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(&stored).map_err(io::Error::other)?;
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &file)
    })();
    if let Err(e) = written {
        eprintln!(
            "RequestTailor: cannot remember the collections in {}: {e}",
            file.display()
        );
    }
}

// ---- commands ----

type CmdResult<T> = std::result::Result<T, CommandError>;

pub(crate) async fn pick_folder(
    app: tauri::AppHandle,
    title: &'static str,
) -> CmdResult<Option<PathBuf>> {
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::Shell(e.to_string()))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    Ok(Some(picked.into_path().map_err(|e| {
        AppError::Shell(format!("unsupported folder: {e}"))
    })?))
}

#[tauri::command]
pub async fn collections_current(core: State<'_, AppCore>) -> CmdResult<CollectionSet> {
    Ok(core.collection_set())
}

/// "Add collection…": the native folder picker, then the folder joins the set.
#[tauri::command]
pub async fn collections_add(
    app: tauri::AppHandle,
    core: State<'_, AppCore>,
) -> CmdResult<Option<CollectionSet>> {
    let Some(path) = pick_folder(app, "Add a folder with collections").await? else {
        return Ok(None);
    };
    Ok(Some(core.add_root(&path)?))
}

#[tauri::command]
pub async fn collections_remove_root(
    core: State<'_, AppCore>,
    root: String,
) -> CmdResult<CollectionSet> {
    Ok(core.remove_root(&root)?)
}

#[tauri::command]
pub async fn collections_activate(
    core: State<'_, AppCore>,
    id: String,
) -> CmdResult<CollectionInfo> {
    Ok(core.activate(&id)?)
}

#[tauri::command]
pub async fn collections_tree(core: State<'_, AppCore>, id: String) -> CmdResult<TreeNode> {
    Ok(core.tree_of(&id)?)
}

#[tauri::command]
pub async fn collections_dirty(core: State<'_, AppCore>, id: String) -> CmdResult<Vec<String>> {
    Ok(core.dirty_of(&id)?)
}

#[tauri::command]
pub async fn collections_save_all(core: State<'_, AppCore>) -> CmdResult<Vec<CollectionChanges>> {
    Ok(core.save_all()?)
}

#[tauri::command]
pub async fn collections_search(
    core: State<'_, AppCore>,
    query: String,
    limit: usize,
) -> CmdResult<Vec<CollectionSearchHit>> {
    Ok(core.search_all(&query, limit)?)
}

/// Creates a collection (unsaved). Without `spec.root` the folder picker asks where (`None`
/// if cancelled): the chosen folder joins the set as a root, the collection goes into
/// `spec.dir` inside it.
#[tauri::command]
pub async fn collections_create(
    app: tauri::AppHandle,
    core: State<'_, AppCore>,
    spec: NewCollection,
) -> CmdResult<Option<CollectionInfo>> {
    let mut spec = spec;
    if spec.root.is_none() {
        let Some(path) = pick_folder(app, "Choose a folder for the new collection").await? else {
            return Ok(None);
        };
        spec.root = Some(core.add_folder_root(&path)?);
    }
    Ok(Some(core.create_collection(&spec)?))
}

/// Shows the folder of collection `id` in the system's file manager (Finder, Explorer, the
/// desktop's default). Runs on the Rust side: the webview has no shell or opener permission.
/// A new collection that was not saved yet has no folder: refused with a clear message.
#[tauri::command]
pub fn collections_reveal(core: State<'_, AppCore>, id: String) -> CmdResult<()> {
    // Wave 15: also a repository of the set (its id is its folder).
    let path = core
        .path_of(&id)
        .or_else(|e| core.repository(&id).map(|r| PathBuf::from(r.id)).map_err(|_| e))?;
    if !path.is_dir() {
        return Err(AppError::Shell(
            "save the new collection first; its folder does not exist yet".into(),
        )
        .into());
    }
    reveal(&path)
        .map_err(|e| AppError::Shell(format!("could not open the file manager: {e}")).into())
}

fn reveal(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg("-R").arg(path);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(path);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    // Not waited for: the file manager keeps running; the child is reaped by the OS.
    cmd.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_empty_folder_is_free_a_file_or_full_folder_is_not() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-free-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(is_free(&dir));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(is_free(&dir));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert!(!is_free(&dir));
        assert!(!is_free(&dir.join("a.txt")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_roots_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("rt-desktop-roots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_roots(&dir), None);
        let set = CollectionSet {
            roots: vec![rt_app::collections::CollectionRoot {
                id: "/a".into(),
                name: "a".into(),
                location: "/a".into(),
                collections: vec!["/a/x".into()],
                truncated: false,
            }],
            collections: Vec::new(),
            active: Some("/a/x".into()),
            repositories: Vec::new(),
            active_repository: None,
        };
        save_roots(&dir, &set);
        assert_eq!(
            load_roots(&dir),
            Some(SavedRoots {
                roots: vec!["/a".into()],
                active: Some("/a/x".into())
            })
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
