// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Semantic merge (`MergeApi`): the field conflicts of the merge in progress, recomputed from the index stages, and their resolution (`rt_app::merges`). The semantic pull itself is in `rt-git` (`merge.rs`).
//!
//! Wave 12, strand B. `merge_conflicts` asks `rt_git::Repo::merge_state` (nothing is stored:
//! the index stages are the state, so a merge from the command line shows here too);
//! `merge_resolve` refuses while the workspace has unsaved changes, writes the files the
//! choices decide (`Repo::resolve_merge`) and reopens the collection. The next `git_commit`
//! concludes the merge.

use std::path::PathBuf;

use rt_app::merges::{ConflictChoice, MergeState};
use rt_app::{AppError, SecretStore};
use rt_git::{Options, Repo, RepoExt as _};

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

type Result<T, E = AppError> = std::result::Result<T, E>;

fn root<S: SecretStore>(core: &Core<S>) -> Result<PathBuf> {
    core.root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))
}

/// The merge state of the open collection; none outside a repository.
pub fn conflicts<S: SecretStore>(core: &Core<S>) -> Result<MergeState> {
    match Repo::open(&root(core)?, &Options::default())? {
        Some(repo) => Ok(repo.merge_state()?),
        None => Ok(MergeState::default()),
    }
}

pub fn resolve<S: SecretStore>(core: &Core<S>, choices: &[ConflictChoice]) -> Result<()> {
    if core.has_unsaved_changes() {
        return Err(AppError::Git(
            "save or discard your changes before you resolve a conflict".into(),
        ));
    }
    let repo = Repo::require(&root(core)?, &Options::default())?;
    repo.resolve_merge(choices)?;
    let remaining = !repo.merge_state()?.conflicts.is_empty()
        || repo
            .status()?
            .files
            .iter()
            .any(|f| f.state == rt_app::git::GitFileState::Conflicted);
    match core.reload() {
        Ok(_) => Ok(()),
        // Other files may still carry conflict markers.
        Err(_) if remaining => Ok(()),
        Err(e) => Err(e),
    }
}

async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppCore) -> Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    use tauri::Manager as _;
    tokio::task::spawn_blocking(move || f(&app.state::<AppCore>()))
        .await
        .map_err(|e| AppError::Shell(format!("the merge failed: {e}")))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn merge_conflicts(app: tauri::AppHandle) -> Result<MergeState, CommandError> {
    blocking(app, conflicts).await
}

#[tauri::command]
pub async fn merge_resolve(
    app: tauri::AppHandle,
    choices: Vec<ConflictChoice>,
) -> Result<(), CommandError> {
    blocking(app, move |core| resolve(core, &choices)).await
}
