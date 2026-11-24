// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The index as building blocks (`GitApi.indexDiff`, `stage`, `unstage`, `stagePatch`,
//! `unstagePatch`, `discardPatch`, `commitStaged`; `rt_app::git::index`), signature checks
//! (`GitApi.verify`, `rt_app::git::signing`) and `GitApi.config`. Wave 15, strand F: natively
//! in `rt-git`.
//!
//! The active repository's `Repo` carries the distribution's `rt_git::sign::Signing`
//! (managed state read with `try_state` at startup, `Core::signing`): `commitStaged` and every
//! other commit the app writes go through its signer; `verify` asks its verifier, or answers
//! `unverified`/`unsigned` without one. These calls refuse while the app has unsaved changes
//! only where they would change files the app holds (`discardPatch`).

use rt_app::git::GitCommit;
use rt_app::git::index::{GitIndexDiff, GitPatchRequest, GitPatchSide, GitStagedCommit};
use rt_app::git::signing::GitSignatureCheck;
use rt_app::{AppError, SecretStore};

use crate::commands::CommandError;
use crate::core::Core;
use crate::git::{app_identity, blocking, repo};

type Result<T, E = AppError> = std::result::Result<T, E>;

pub fn index_diff<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    side: GitPatchSide,
) -> Result<GitIndexDiff> {
    repo(core)?.index_diff(path, side)
}

pub fn stage<S: SecretStore>(core: &Core<S>, paths: &[String]) -> Result<()> {
    repo(core)?.stage(paths)
}

pub fn unstage<S: SecretStore>(core: &Core<S>, paths: &[String]) -> Result<()> {
    repo(core)?.unstage(paths)
}

pub fn stage_patch<S: SecretStore>(core: &Core<S>, patch: &GitPatchRequest) -> Result<()> {
    repo(core)?.stage_patch(patch)
}

pub fn unstage_patch<S: SecretStore>(core: &Core<S>, patch: &GitPatchRequest) -> Result<()> {
    repo(core)?.unstage_patch(patch)
}

pub fn discard_patch<S: SecretStore>(core: &Core<S>, patch: &GitPatchRequest) -> Result<()> {
    if core.has_unsaved_changes() {
        return Err(AppError::Git(
            "save or discard your changes before you discard lines".into(),
        ));
    }
    repo(core)?.discard_patch(patch)?;
    // A collection file changed on disk: reload what the app holds.
    let _ = core.reload();
    Ok(())
}

pub fn commit_staged<S: SecretStore>(
    core: &Core<S>,
    commit: &GitStagedCommit,
) -> Result<GitCommit> {
    let fallback = app_identity(core)?;
    repo(core)?.commit_staged(commit, fallback.as_ref())
}

pub fn verify<S: SecretStore>(core: &Core<S>, oids: &[String]) -> Result<Vec<GitSignatureCheck>> {
    repo(core)?.verify(oids)
}

pub fn config<S: SecretStore>(core: &Core<S>, key: &str) -> Result<Option<String>> {
    Ok(repo(core)?.config_value(key))
}

#[tauri::command]
pub async fn git_index_diff(
    app: tauri::AppHandle,
    path: String,
    side: GitPatchSide,
) -> Result<GitIndexDiff, CommandError> {
    blocking(app, move |core| index_diff(core, &path, side)).await
}

#[tauri::command]
pub async fn git_stage(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), CommandError> {
    blocking(app, move |core| stage(core, &paths)).await
}

#[tauri::command]
pub async fn git_unstage(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), CommandError> {
    blocking(app, move |core| unstage(core, &paths)).await
}

#[tauri::command]
pub async fn git_stage_patch(
    app: tauri::AppHandle,
    patch: GitPatchRequest,
) -> Result<(), CommandError> {
    blocking(app, move |core| stage_patch(core, &patch)).await
}

#[tauri::command]
pub async fn git_unstage_patch(
    app: tauri::AppHandle,
    patch: GitPatchRequest,
) -> Result<(), CommandError> {
    blocking(app, move |core| unstage_patch(core, &patch)).await
}

#[tauri::command]
pub async fn git_discard_patch(
    app: tauri::AppHandle,
    patch: GitPatchRequest,
) -> Result<(), CommandError> {
    blocking(app, move |core| discard_patch(core, &patch)).await
}

#[tauri::command]
pub async fn git_commit_staged(
    app: tauri::AppHandle,
    commit: GitStagedCommit,
) -> Result<GitCommit, CommandError> {
    blocking(app, move |core| commit_staged(core, &commit)).await
}

#[tauri::command]
pub async fn git_verify(
    app: tauri::AppHandle,
    oids: Vec<String>,
) -> Result<Vec<GitSignatureCheck>, CommandError> {
    blocking(app, move |core| verify(core, &oids)).await
}

#[tauri::command]
pub async fn git_config(
    app: tauri::AppHandle,
    key: String,
) -> Result<Option<String>, CommandError> {
    blocking(app, move |core| config(core, &key)).await
}
