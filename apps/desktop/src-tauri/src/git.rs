// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Git commands of the desktop app (`GitApi` in `apps/ui/src/commands/backend.ts`), backed by
//! the `rt-git` crate (gitoxide) on [`crate::core::Core::root`].
//!
//! The functions here take a [`Core`] and hold the shell's part of the rules, so they can be
//! tested without a window (`tests/git*.rs`); the Tauri commands below only run them on a
//! blocking thread (Git does file and network I/O):
//!
//! - `checkout`, `pull` and `resolve` refuse while the workspace has unsaved changes
//!   (`AppError::Git`) and reopen the collection afterwards (`Core::reload`). After a pull
//!   that stopped on conflicts the files hold conflict markers and may not parse; the old
//!   state stays open until the conflicts are resolved.
//! - Credentials live in the secret store (`Core::secrets`, the keychain in the app) under
//!   the scheme of `rt_git::credentials`.
//! - The app's own author identity (for when the Git configuration has none) is a JSON file
//!   in `Core::data_dir`.

use std::path::{Path, PathBuf};

use rt_app::git::{
    GitBranch, GitCommit, GitConflict, GitDiff, GitIdentity, GitPullResult, GitResolution,
    GitStatus,
};
use rt_app::{AppError, SecretStore};
use rt_git::{Options, Repo, StoreCredentials};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{AppCore, CommandError};
use crate::core::{CollectionInfo, Core};

type Result<T, E = AppError> = std::result::Result<T, E>;

fn options() -> Options {
    Options::default()
}

fn root<S: SecretStore>(core: &Core<S>) -> Result<PathBuf> {
    core.root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))
}

fn repo<S: SecretStore>(core: &Core<S>) -> Result<Repo> {
    Repo::require(&root(core)?, &options())
}

fn refuse_unsaved<S: SecretStore>(core: &Core<S>, what: &str) -> Result<()> {
    if core.has_unsaved_changes() {
        return Err(AppError::Git(format!(
            "save or discard your changes before you {what}"
        )));
    }
    Ok(())
}

fn app_identity<S: SecretStore>(core: &Core<S>) -> Result<Option<GitIdentity>> {
    rt_git::identity::load(core.data_dir.as_deref())
}

pub fn status<S: SecretStore>(core: &Core<S>) -> Result<GitStatus> {
    rt_git::status(&root(core)?, &options())
}

pub fn diff<S: SecretStore>(core: &Core<S>, path: &str) -> Result<GitDiff> {
    repo(core)?.diff(path)
}

pub fn commit<S: SecretStore>(
    core: &Core<S>,
    message: &str,
    paths: &[String],
) -> Result<GitCommit> {
    let fallback = app_identity(core)?;
    repo(core)?.commit(message, paths, fallback.as_ref())
}

pub fn log<S: SecretStore>(core: &Core<S>, limit: u32) -> Result<Vec<GitCommit>> {
    repo(core)?.log(limit)
}

pub fn branches<S: SecretStore>(core: &Core<S>) -> Result<Vec<GitBranch>> {
    repo(core)?.branches()
}

pub fn checkout<S: SecretStore>(core: &Core<S>, branch: &str) -> Result<()> {
    refuse_unsaved(core, "switch branches")?;
    repo(core)?.checkout(branch)?;
    core.reload()?;
    Ok(())
}

/// Creating a branch never changes files (with `checkout` both point at the same commit),
/// so unsaved changes do not matter here.
pub fn create_branch<S: SecretStore>(core: &Core<S>, name: &str, checkout: bool) -> Result<()> {
    repo(core)?.create_branch(name, checkout)
}

pub fn push<S: SecretStore>(core: &Core<S>) -> Result<()> {
    repo(core)?.push(&StoreCredentials(core.secrets()))
}

pub fn pull<S: SecretStore>(core: &Core<S>) -> Result<GitPullResult> {
    refuse_unsaved(core, "pull")?;
    let fallback = app_identity(core)?;
    let result = repo(core)?.pull(&StoreCredentials(core.secrets()), fallback.as_ref())?;
    match &result {
        GitPullResult::UpToDate => {}
        // Files with conflict markers may not parse: keep what is open until resolved.
        GitPullResult::Conflicts { .. } => {
            let _ = core.reload();
        }
        GitPullResult::FastForward { .. } | GitPullResult::Merged { .. } => {
            core.reload()?;
        }
    }
    Ok(result)
}

pub fn conflict<S: SecretStore>(core: &Core<S>, path: &str) -> Result<GitConflict> {
    repo(core)?.conflict(path)
}

pub fn resolve<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    resolution: GitResolution,
) -> Result<()> {
    refuse_unsaved(core, "resolve a conflict")?;
    let repo = repo(core)?;
    repo.resolve(path, resolution)?;
    let remaining = repo
        .status()?
        .files
        .iter()
        .any(|f| f.state == rt_app::git::GitFileState::Conflicted);
    match core.reload() {
        Ok(_) => Ok(()),
        // Other files still carry conflict markers.
        Err(_) if remaining => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn init<S: SecretStore>(core: &Core<S>) -> Result<()> {
    rt_git::init(&root(core)?, &options())
}

/// Clones `url` into `<parent>/<repository name>` and opens it.
pub fn clone_into<S: SecretStore>(
    core: &Core<S>,
    url: &str,
    parent: &Path,
) -> Result<CollectionInfo> {
    let dir = rt_git::clone(url, parent, &StoreCredentials(core.secrets()), &options())?;
    core.open(&dir)
}

/// The Git configuration's identity (repository, then global), else the app's own.
pub fn identity<S: SecretStore>(core: &Core<S>) -> Result<Option<GitIdentity>> {
    let configured = match core.root() {
        Some(root) => match Repo::open(&root, &options())? {
            Some(repo) => repo.config_identity(),
            None => rt_git::global_identity(&options()),
        },
        None => rt_git::global_identity(&options()),
    };
    match configured {
        Some(id) => Ok(Some(id)),
        None => app_identity(core),
    }
}

pub fn set_identity<S: SecretStore>(core: &Core<S>, identity: &GitIdentity) -> Result<()> {
    rt_git::identity::save(core.data_dir.as_deref(), identity)
}

pub fn has_credentials<S: SecretStore>(core: &Core<S>, host: &str) -> Result<bool> {
    rt_git::credentials::has(core.secrets(), host)
}

pub fn set_credentials<S: SecretStore>(
    core: &Core<S>,
    host: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    rt_git::credentials::set(core.secrets(), host, username, password)
}

pub fn delete_credentials<S: SecretStore>(core: &Core<S>, host: &str) -> Result<()> {
    rt_git::credentials::delete(core.secrets(), host)
}

/// Runs `f` on a blocking thread (Git does file and network I/O; reqwest's blocking client
/// must not run on the async runtime).
async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppCore) -> Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    use tauri::Manager as _;
    tokio::task::spawn_blocking(move || f(&app.state::<AppCore>()))
        .await
        .map_err(|e| AppError::Shell(format!("Git failed: {e}")))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn git_status(app: tauri::AppHandle) -> Result<GitStatus, CommandError> {
    blocking(app, status).await
}

#[tauri::command]
pub async fn git_diff(app: tauri::AppHandle, path: String) -> Result<GitDiff, CommandError> {
    blocking(app, move |core| diff(core, &path)).await
}

#[tauri::command]
pub async fn git_commit(
    app: tauri::AppHandle,
    message: String,
    paths: Vec<String>,
) -> Result<GitCommit, CommandError> {
    blocking(app, move |core| commit(core, &message, &paths)).await
}

#[tauri::command]
pub async fn git_log(app: tauri::AppHandle, limit: u32) -> Result<Vec<GitCommit>, CommandError> {
    blocking(app, move |core| log(core, limit)).await
}

#[tauri::command]
pub async fn git_branches(app: tauri::AppHandle) -> Result<Vec<GitBranch>, CommandError> {
    blocking(app, branches).await
}

#[tauri::command]
pub async fn git_checkout(app: tauri::AppHandle, branch: String) -> Result<(), CommandError> {
    blocking(app, move |core| checkout(core, &branch)).await
}

#[tauri::command]
pub async fn git_create_branch(
    app: tauri::AppHandle,
    name: String,
    checkout: bool,
) -> Result<(), CommandError> {
    blocking(app, move |core| create_branch(core, &name, checkout)).await
}

#[tauri::command]
pub async fn git_push(app: tauri::AppHandle) -> Result<(), CommandError> {
    blocking(app, push).await
}

#[tauri::command]
pub async fn git_pull(app: tauri::AppHandle) -> Result<GitPullResult, CommandError> {
    blocking(app, pull).await
}

#[tauri::command]
pub async fn git_conflict(
    app: tauri::AppHandle,
    path: String,
) -> Result<GitConflict, CommandError> {
    blocking(app, move |core| conflict(core, &path)).await
}

#[tauri::command]
pub async fn git_resolve(
    app: tauri::AppHandle,
    path: String,
    resolution: GitResolution,
) -> Result<(), CommandError> {
    blocking(app, move |core| resolve(core, &path, resolution)).await
}

#[tauri::command]
pub async fn git_init(app: tauri::AppHandle) -> Result<(), CommandError> {
    blocking(app, init).await
}

/// Shows the folder picker, clones `url` into a new folder inside the picked one and opens
/// it; `None` if cancelled.
#[tauri::command]
pub async fn git_clone(
    app: tauri::AppHandle,
    url: String,
) -> Result<Option<CollectionInfo>, CommandError> {
    let picker = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        picker
            .dialog()
            .file()
            .set_title("Choose where to clone the repository")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::Shell(e.to_string()))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let parent = picked
        .into_path()
        .map_err(|e| AppError::Shell(format!("unsupported folder: {e}")))?;
    blocking(app, move |core| clone_into(core, &url, &parent).map(Some)).await
}

#[tauri::command]
pub async fn git_identity(app: tauri::AppHandle) -> Result<Option<GitIdentity>, CommandError> {
    blocking(app, identity).await
}

#[tauri::command]
pub async fn git_set_identity(
    app: tauri::AppHandle,
    identity: GitIdentity,
) -> Result<(), CommandError> {
    blocking(app, move |core| set_identity(core, &identity)).await
}

#[tauri::command]
pub async fn git_has_credentials(
    app: tauri::AppHandle,
    host: String,
) -> Result<bool, CommandError> {
    blocking(app, move |core| has_credentials(core, &host)).await
}

#[tauri::command]
pub async fn git_set_credentials(
    app: tauri::AppHandle,
    host: String,
    username: String,
    password: String,
) -> Result<(), CommandError> {
    blocking(app, move |core| {
        set_credentials(core, &host, &username, &password)
    })
    .await
}

#[tauri::command]
pub async fn git_delete_credentials(
    app: tauri::AppHandle,
    host: String,
) -> Result<(), CommandError> {
    blocking(app, move |core| delete_credentials(core, &host)).await
}
