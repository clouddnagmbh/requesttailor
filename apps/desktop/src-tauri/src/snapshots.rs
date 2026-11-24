// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Snapshot tests for the desktop app (`Backend.snapshots`, `rt_app::snapshots`), answered from
//! the active collection: companion files and the assertion are staged in the workspace
//! (unsaved), the kept responses are read from the response store. The stored body still has
//! the secret values of its send, so the commands read the secrets of the response's
//! environment from the keychain, only to mask them (the webview never sends secrets).

use rt_app::snapshots::{
    self, FreezeOptions, Snapshot, SnapshotCheck, SnapshotEntry, SnapshotFreeze,
};
use rt_app::{AppError, SecretStore, SendInput};
use tauri::State;

use crate::Core;
use crate::commands::{AppCore, CommandError};

/// The secret values of the environment the stored response was sent with, as plain strings.
fn secrets_of<S: SecretStore>(core: &Core<S>, response_id: u64) -> Result<Vec<String>, AppError> {
    let environment =
        core.with_responses(|_, responses| snapshots::environment_of(responses, response_id))?;
    core.with_ai_scope(
        SendInput {
            environment,
            ..SendInput::default()
        },
        |_, _, input, _| input.secrets.iter().map(|(_, v)| v.clone()).collect(),
    )
}

/// The snapshot of the request at `path`; `None` if it has none.
pub fn read<S: SecretStore>(core: &Core<S>, path: &str) -> Result<Option<Snapshot>, AppError> {
    core.with_workspace(|ws, _| snapshots::read(ws, path))
}

/// Freezes the kept response as the request's snapshot (staged, unsaved).
pub fn freeze<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    response_id: u64,
    options: &FreezeOptions,
) -> Result<SnapshotFreeze, AppError> {
    let secrets = secrets_of(core, response_id)?;
    let plan = core.with_responses(|ws, responses| {
        snapshots::plan_freeze(ws, responses, path, response_id, options, &secrets)
    })?;
    core.with_workspace(|ws, _| snapshots::apply_freeze(ws, plan))
}

pub fn set_ignore<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    ignore: Vec<String>,
) -> Result<Snapshot, AppError> {
    core.with_workspace(|ws, _| snapshots::set_ignore(ws, path, ignore))
}

pub fn check<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    response_id: u64,
) -> Result<SnapshotCheck, AppError> {
    let secrets = secrets_of(core, response_id)?;
    core.with_responses(|ws, responses| {
        snapshots::check(ws, responses, path, response_id, &secrets)
    })
}

pub fn list<S: SecretStore>(core: &Core<S>) -> Result<Vec<SnapshotEntry>, AppError> {
    core.with_workspace(|ws, _| Ok(snapshots::list(ws)))
}

#[tauri::command]
pub async fn snapshots_read(
    core: State<'_, AppCore>,
    path: String,
) -> Result<Option<Snapshot>, CommandError> {
    Ok(read(&core, &path)?)
}

#[tauri::command]
pub async fn snapshots_freeze(
    core: State<'_, AppCore>,
    path: String,
    response_id: u64,
    options: FreezeOptions,
) -> Result<SnapshotFreeze, CommandError> {
    Ok(freeze(&core, &path, response_id, &options)?)
}

#[tauri::command]
pub async fn snapshots_set_ignore(
    core: State<'_, AppCore>,
    path: String,
    ignore: Vec<String>,
) -> Result<Snapshot, CommandError> {
    Ok(set_ignore(&core, &path, ignore)?)
}

#[tauri::command]
pub async fn snapshots_check(
    core: State<'_, AppCore>,
    path: String,
    response_id: u64,
) -> Result<SnapshotCheck, CommandError> {
    Ok(check(&core, &path, response_id)?)
}

#[tauri::command]
pub async fn snapshots_list(core: State<'_, AppCore>) -> Result<Vec<SnapshotEntry>, CommandError> {
    Ok(list(&core)?)
}
