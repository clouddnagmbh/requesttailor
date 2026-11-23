// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Repositories in the collection set (`Backend.repositories`, `rt_app::git::repos`), wave 15
//! strand A. Any Git repository can be opened, not only one that holds a collection: a folder
//! without a collection in Git joins the set as a *repository root* ([`Core::add_root`]), the
//! active repository lives in [`Core`] (`active_repository`, what every `git_*` command acts
//! on), and clone and init add the new folder as a root.
//!
//! The functions take the core, so the tests drive them without a window; the Tauri commands
//! only ask for folders and forward.

use std::path::{Path, PathBuf};

use rt_app::collections::CollectionSet;
use rt_app::git::repos::{
    GitCloneRequest, GitOperation, GitRepoSummary, RepositoryEntry, summarize,
};
use rt_app::{AppError, SecretStore};
use rt_git::{Options, Repo, StoreCredentials};

use crate::collections::pick_folder;
use crate::commands::CommandError;
use crate::core::Core;
use crate::git::blocking;

type Result<T, E = AppError> = std::result::Result<T, E>;

/// A folder joins the set: its collections, or, without any, the Git repository it is in as a
/// repository root; outside Git a folder without a collection is a new collection, as
/// `collections.add` does.
pub fn open<S: SecretStore>(core: &Core<S>, folder: &Path) -> Result<CollectionSet> {
    core.add_root(folder)
}

/// Makes repository `id` the active one.
pub fn activate<S: SecretStore>(core: &Core<S>, id: &str) -> Result<RepositoryEntry> {
    core.activate_repository(id)
}

/// What the sidebar shows for repository `id`: branch, ahead/behind, changed files of the
/// whole working tree and a half-done operation. The id is the working tree's path.
pub fn summary<S: SecretStore>(core: &Core<S>, id: &str) -> Result<GitRepoSummary> {
    let entry = core.repository(id)?;
    let path = PathBuf::from(&entry.id);
    let status = rt_git::status(&path, &Options::default())?;
    let operation = status
        .merging
        .then(|| underway(&path))
        .flatten()
        .or_else(|| rebase_underway(&path).then_some(GitOperation::Rebase));
    Ok(summarize(&entry.id, &status, operation))
}

/// The git directory of the working tree `work`: `.git`, or where a linked worktree's or a
/// submodule's `.git` file points.
fn git_dir(work: &Path) -> Option<PathBuf> {
    let dot = work.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let target = Path::new(target);
    Some(if target.is_absolute() {
        target.to_path_buf()
    } else {
        work.join(target)
    })
}

fn underway(work: &Path) -> Option<GitOperation> {
    let dir = git_dir(work)?;
    [
        ("MERGE_HEAD", GitOperation::Merge),
        ("CHERRY_PICK_HEAD", GitOperation::CherryPick),
        ("REVERT_HEAD", GitOperation::Revert),
    ]
    .into_iter()
    .find(|(file, _)| dir.join(file).is_file())
    .map(|(_, op)| op)
}

/// A rebase started on the command line (the app only offers to abort or finish it there).
fn rebase_underway(work: &Path) -> bool {
    git_dir(work)
        .is_some_and(|d| d.join("rebase-merge").is_dir() || d.join("rebase-apply").is_dir())
}

/// A folder name that is one path component and nothing else.
fn plain_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
    {
        return Err(AppError::Git(format!(
            "\"{name}\" is not a folder name; use a plain name"
        )));
    }
    Ok(name)
}

/// Clones `request.url` into `<parent>/<name>` (the name from the address unless the request
/// names one), checks out `request.branch` if asked for, adds the folder to the set and makes
/// its repository active.
pub fn clone<S: SecretStore>(
    core: &Core<S>,
    request: &GitCloneRequest,
    parent: &Path,
) -> Result<RepositoryEntry> {
    let name = match request.name.as_deref().map(str::trim) {
        Some(n) if !n.is_empty() => Some(plain_name(n)?.to_owned()),
        _ => None,
    };
    if let Some(n) = &name
        && parent.join(n).exists()
        && !crate::collections::is_free(&parent.join(n))
    {
        return Err(AppError::Git(format!(
            "the folder \"{n}\" already exists in {}",
            parent.display()
        )));
    }
    let options = Options::default();
    let mut dir = rt_git::clone(
        &request.url,
        parent,
        &StoreCredentials(core.secrets()),
        &options,
    )?;
    if let Some(n) = name
        && dir.file_name().is_some_and(|d| d.to_string_lossy() != n)
    {
        let target = dir.with_file_name(&n);
        // An empty folder of that name is replaced; anything else was refused above.
        let _ = std::fs::remove_dir(&target);
        std::fs::rename(&dir, &target)
            .map_err(|e| AppError::Git(format!("cannot rename the clone to \"{n}\": {e}")))?;
        dir = target;
    }
    if let Some(branch) = request
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        Repo::require(&dir, &options)?.checkout(branch)?;
    }
    added(core, &dir)
}

/// `git init` in `folder` (an existing repository is refused), added to the set as a
/// repository root and made active.
pub fn init<S: SecretStore>(core: &Core<S>, folder: &Path) -> Result<RepositoryEntry> {
    rt_git::init(folder, &Options::default())?;
    added(core, folder)
}

/// Adds `dir` as a root and returns the repository it is in, active.
fn added<S: SecretStore>(core: &Core<S>, dir: &Path) -> Result<RepositoryEntry> {
    let set = core.add_root(dir)?;
    let canonical = rt_git::canonical(dir).map_err(|e| AppError::Git(e.to_string()))?;
    let want = canonical.to_string_lossy().into_owned();
    let entry = set
        .repositories
        .iter()
        .find(|r| r.id == want)
        .or_else(|| set.repositories.last())
        .cloned()
        .ok_or_else(|| AppError::Git(format!("{} is not a Git repository", dir.display())))?;
    // A repository with collections follows the first of them, which `add_root` made active;
    // one without is chosen explicitly.
    if entry.collections.is_empty() {
        core.activate_repository(&entry.id)?;
    }
    Ok(entry)
}

// ---- commands ----

/// Folder picker, then the folder joins the set.
#[tauri::command]
pub async fn repositories_open(
    app: tauri::AppHandle,
) -> Result<Option<CollectionSet>, CommandError> {
    let Some(path) = pick_folder(
        app.clone(),
        "Open a repository or a folder with collections",
    )
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(blocking(app, move |core| open(core, &path)).await?))
}

#[tauri::command]
pub async fn repositories_activate(
    app: tauri::AppHandle,
    id: String,
) -> Result<RepositoryEntry, CommandError> {
    blocking(app, move |core| activate(core, &id)).await
}

#[tauri::command]
pub async fn repositories_summary(
    app: tauri::AppHandle,
    id: String,
) -> Result<GitRepoSummary, CommandError> {
    blocking(app, move |core| summary(core, &id)).await
}

#[tauri::command]
pub async fn repositories_clone(
    app: tauri::AppHandle,
    request: GitCloneRequest,
) -> Result<Option<RepositoryEntry>, CommandError> {
    let parent = match request.parent.clone() {
        Some(p) => PathBuf::from(p),
        None => {
            let Some(p) = pick_folder(app.clone(), "Choose where to clone the repository").await?
            else {
                return Ok(None);
            };
            p
        }
    };
    Ok(Some(
        blocking(app, move |core| clone(core, &request, &parent)).await?,
    ))
}

/// Folder picker, then `git init` there.
#[tauri::command]
pub async fn repositories_init(
    app: tauri::AppHandle,
) -> Result<Option<RepositoryEntry>, CommandError> {
    let Some(path) = pick_folder(app.clone(), "Choose a folder to turn into a repository").await?
    else {
        return Ok(None);
    };
    Ok(Some(blocking(app, move |core| init(core, &path)).await?))
}
