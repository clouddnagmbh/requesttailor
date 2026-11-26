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
//! - `commit` is refused with `AppError::SecretsFound` when the secret scanner
//!   (`rt_app::secret_scan`, wave 5) finds credentials in the files it would take; `rt-git`
//!   runs it as the repository's commit scanner (seam N2). `scan` lists them for the commit
//!   dialog. There is no way around it except marking a line with `# rt-scan:allow`.

use std::path::{Path, PathBuf};

use rt_app::git::{
    GitBranch, GitCommit, GitConflict, GitDiff, GitFileCommit, GitIdentity, GitPullResult,
    GitResolution, GitStatus, GitTree,
};
use rt_app::secret_scan::{self, SecretFinding};
use rt_app::{AppError, SecretStore};
use rt_git::{Options, Repo, StoreCredentials};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{AppCore, CommandError};
use crate::core::{CollectionInfo, Core};

type Result<T, E = AppError> = std::result::Result<T, E>;

fn options() -> Options {
    Options::default()
}

/// The active repository (wave 15: `Core::active_repository`).
fn root<S: SecretStore>(core: &Core<S>) -> Result<PathBuf> {
    core.active_repository()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))
}

pub(crate) fn repo<S: SecretStore>(core: &Core<S>) -> Result<Repo> {
    Ok(Repo::require(&root(core)?, &options())?.with_signing(core.signing.clone()))
}

fn refuse_unsaved<S: SecretStore>(core: &Core<S>, what: &str) -> Result<()> {
    if core.has_unsaved_changes() {
        return Err(AppError::Git(format!(
            "save or discard your changes before you {what}"
        )));
    }
    Ok(())
}

pub(crate) fn app_identity<S: SecretStore>(core: &Core<S>) -> Result<Option<GitIdentity>> {
    Ok(rt_git::identity::load(core.data_dir.as_deref())?)
}

pub fn status<S: SecretStore>(core: &Core<S>) -> Result<GitStatus> {
    Ok(rt_git::status(&root(core)?, &options())?)
}

pub fn diff<S: SecretStore>(core: &Core<S>, path: &str) -> Result<GitDiff> {
    Ok(repo(core)?.diff(path)?)
}

/// Credentials typed into `paths` (relative to the collection root), as the files are in the
/// working tree. Deleted files are skipped, as are binary files and files over 4 MiB.
pub fn scan<S: SecretStore>(core: &Core<S>, paths: &[String]) -> Result<Vec<SecretFinding>> {
    let root = root(core)?;
    let mut texts = Vec::new();
    for path in paths {
        let relative = Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(AppError::Git(format!(
                "not a path in the collection: {path}"
            )));
        }
        let bytes = match std::fs::read(root.join(relative)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(AppError::Shell(format!("cannot read {path}: {e}"))),
        };
        if let Some(text) = secret_scan::text_of(&bytes) {
            texts.push((path.as_str(), text.to_owned()));
        }
    }
    let files: Vec<(&str, &str)> = texts.iter().map(|(p, t)| (*p, t.as_str())).collect();
    Ok(secret_scan::scan(&files))
}

pub fn commit<S: SecretStore>(
    core: &Core<S>,
    message: &str,
    paths: &[String],
) -> Result<GitCommit> {
    let fallback = app_identity(core)?;
    Ok(repo(core)?.commit(message, paths, fallback.as_ref())?)
}

pub fn log<S: SecretStore>(core: &Core<S>, limit: u32) -> Result<Vec<GitCommit>> {
    Ok(repo(core)?.log(limit)?)
}

pub fn branches<S: SecretStore>(core: &Core<S>) -> Result<Vec<GitBranch>> {
    Ok(repo(core)?.branches()?)
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
    Ok(repo(core)?.create_branch(name, checkout)?)
}

pub fn push<S: SecretStore>(core: &Core<S>) -> Result<()> {
    Ok(repo(core)?.push(&StoreCredentials(core.secrets()))?)
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

/// After Git changed files of the repository at `workdir` (wave 14: the wizards' actions and
/// undo), reopens every open collection in it (`Core::reload` reads the collections of the
/// active collection's repository again). `conflicts`: files may hold conflict markers and
/// not parse; then what is open stays until the conflicts are resolved.
pub fn reload_repository<S: SecretStore>(
    core: &Core<S>,
    workdir: &Path,
    conflicts: bool,
) -> Result<()> {
    // Wave 15: the active repository (the chosen one, else the active collection's).
    let Some(root) = core.active_repository() else {
        return Ok(());
    };
    let inside = rt_git::canonical(&root)
        .map(|r| r.starts_with(workdir))
        .unwrap_or(false);
    if !inside {
        return Ok(());
    }
    match core.reload() {
        Ok(_) => Ok(()),
        Err(_) if conflicts => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn conflict<S: SecretStore>(core: &Core<S>, path: &str) -> Result<GitConflict> {
    Ok(repo(core)?.conflict(path)?)
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
    Ok(rt_git::init(&root(core)?, &options())?)
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
    Ok(rt_git::identity::save(core.data_dir.as_deref(), identity)?)
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

// ---- Wave 12 (interface): revisions, read without a checkout ----

pub fn rev_parse<S: SecretStore>(core: &Core<S>, rev: &str) -> Result<String> {
    Ok(repo(core)?.rev_parse(rev)?)
}

pub fn tree_at<S: SecretStore>(core: &Core<S>, rev: &str) -> Result<GitTree> {
    Ok(repo(core)?.tree_at(rev)?)
}

pub fn show<S: SecretStore>(core: &Core<S>, rev: &str, path: &str) -> Result<Option<String>> {
    Ok(repo(core)?.show(rev, path)?)
}

pub fn file_log<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    limit: u32,
) -> Result<Vec<GitFileCommit>> {
    Ok(repo(core)?.file_log(path, limit)?)
}

pub fn merge_base<S: SecretStore>(core: &Core<S>, a: &str, b: &str) -> Result<Option<String>> {
    Ok(repo(core)?.merge_base(a, b)?)
}

pub fn commits<S: SecretStore>(
    core: &Core<S>,
    from: Option<&str>,
    to: &str,
    limit: u32,
) -> Result<Vec<GitCommit>> {
    Ok(repo(core)?.commits(from, to, limit)?)
}

/// Runs `f` on a blocking thread (Git does file and network I/O; reqwest's blocking client
/// must not run on the async runtime).
pub(crate) async fn blocking<T: Send + 'static>(
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

/// Shows the folder picker for a repository on this machine (the folder of a normal
/// repository or a bare one) and returns its path; `None` if cancelled.
#[tauri::command]
pub async fn git_pick_repository(app: tauri::AppHandle) -> Result<Option<String>, CommandError> {
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose the repository to clone")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::Shell(e.to_string()))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| AppError::Shell(format!("unsupported folder: {e}")))?;
    Ok(Some(path.to_string_lossy().into_owned()))
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

/// Credentials typed into `paths` of the collection (wave 5, strand E: [`scan`];
/// `git_commit` refuses with `secrets-found` when this finds any).
#[tauri::command]
pub async fn git_scan(
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<Vec<SecretFinding>, CommandError> {
    blocking(app, move |core| scan(core, &paths)).await
}

#[tauri::command]
pub async fn git_rev_parse(app: tauri::AppHandle, rev: String) -> Result<String, CommandError> {
    blocking(app, move |core| rev_parse(core, &rev)).await
}

#[tauri::command]
pub async fn git_tree_at(app: tauri::AppHandle, rev: String) -> Result<GitTree, CommandError> {
    blocking(app, move |core| tree_at(core, &rev)).await
}

#[tauri::command]
pub async fn git_show(
    app: tauri::AppHandle,
    rev: String,
    path: String,
) -> Result<Option<String>, CommandError> {
    blocking(app, move |core| show(core, &rev, &path)).await
}

#[tauri::command]
pub async fn git_file_log(
    app: tauri::AppHandle,
    path: String,
    limit: u32,
) -> Result<Vec<GitFileCommit>, CommandError> {
    blocking(app, move |core| file_log(core, &path, limit)).await
}

#[tauri::command]
pub async fn git_merge_base(
    app: tauri::AppHandle,
    a: String,
    b: String,
) -> Result<Option<String>, CommandError> {
    blocking(app, move |core| merge_base(core, &a, &b)).await
}

#[tauri::command]
pub async fn git_commits(
    app: tauri::AppHandle,
    from: Option<String>,
    to: String,
    limit: u32,
) -> Result<Vec<GitCommit>, CommandError> {
    blocking(app, move |core| commits(core, from.as_deref(), &to, limit)).await
}
