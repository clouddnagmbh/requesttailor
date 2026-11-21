// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Tauri commands: one per `Backend` method (`apps/ui/src/commands/backend.ts`), forwarding to
//! [`Core`]. Arguments arrive camelCase from JavaScript (`requestPath` → `request_path`); the
//! TypeScript side is `apps/desktop/src/backend.ts`. Errors are the serialized `AppError`
//! (`{ kind, message }`), which the TypeScript side rethrows as `BackendError`.
//!
//! Every command is `async`, so none runs on the main (UI) thread.

use rt_app::{
    AppError, FileChange, LineSlice, Preview, SendInput, StoredResponse, TreeNode,
    secrets::keychain::KeychainStore,
};
use rt_core::model::{Environment, RequestSpec};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::core::{CollectionInfo, Core};

pub type AppCore = Core<KeychainStore>;
type Result<T> = std::result::Result<T, AppError>;

/// Shows the native folder picker and opens the chosen folder; `None` if cancelled.
#[tauri::command]
pub async fn open_collection(
    app: tauri::AppHandle,
    core: State<'_, AppCore>,
) -> Result<Option<CollectionInfo>> {
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Open collection folder")
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
    core.open(&path).map(Some)
}

#[tauri::command]
pub async fn tree(core: State<'_, AppCore>) -> Result<TreeNode> {
    core.tree()
}

#[tauri::command]
pub async fn get_request(core: State<'_, AppCore>, path: String) -> Result<RequestSpec> {
    core.get_request(&path)
}

#[tauri::command]
pub async fn create_request(
    core: State<'_, AppCore>,
    folder: String,
    name: String,
) -> Result<String> {
    core.create_request(&folder, &name)
}

#[tauri::command]
pub async fn update_request(
    core: State<'_, AppCore>,
    path: String,
    spec: RequestSpec,
) -> Result<()> {
    core.update_request(&path, spec)
}

#[tauri::command]
pub async fn dirty(core: State<'_, AppCore>) -> Result<Vec<String>> {
    core.dirty()
}

#[tauri::command]
pub async fn save(core: State<'_, AppCore>) -> Result<Vec<FileChange>> {
    core.save()
}

#[tauri::command]
pub async fn environments(core: State<'_, AppCore>) -> Result<Vec<Environment>> {
    core.environments()
}

#[tauri::command]
pub async fn update_environment(core: State<'_, AppCore>, env: Environment) -> Result<()> {
    core.update_environment(env)
}

#[tauri::command]
pub async fn delete_environment(core: State<'_, AppCore>, name: String) -> Result<()> {
    core.delete_environment(&name)
}

#[tauri::command]
pub async fn preview(core: State<'_, AppCore>, path: String, input: SendInput) -> Result<Preview> {
    core.preview(&path, input)
}

#[tauri::command]
pub async fn send(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<StoredResponse> {
    core.send(&path, input).await
}

#[tauri::command]
pub async fn response_lines(
    core: State<'_, AppCore>,
    id: u64,
    start: u64,
    count: u32,
) -> Result<LineSlice> {
    core.response_lines(id, start, count)
}

/// Raw bytes, sent as binary IPC (an `ArrayBuffer` in JavaScript), not as a JSON array.
#[tauri::command]
pub async fn response_bytes(
    core: State<'_, AppCore>,
    id: u64,
    offset: u64,
    length: u32,
) -> Result<tauri::ipc::Response> {
    core.response_bytes(id, offset, length)
        .map(tauri::ipc::Response::new)
}

#[tauri::command]
pub async fn filter_json(core: State<'_, AppCore>, id: u64, expression: String) -> Result<String> {
    core.filter_json(id, &expression)
}

#[tauri::command]
pub async fn release_response(core: State<'_, AppCore>, id: u64) -> Result<()> {
    core.release_response(id);
    Ok(())
}

#[tauri::command]
pub async fn secret_has(
    core: State<'_, AppCore>,
    environment: String,
    variable: String,
) -> Result<bool> {
    core.secret_has(&environment, &variable)
}

#[tauri::command]
pub async fn secret_get(
    core: State<'_, AppCore>,
    environment: String,
    variable: String,
) -> Result<Option<String>> {
    core.secret_get(&environment, &variable)
}

#[tauri::command]
pub async fn secret_set(
    core: State<'_, AppCore>,
    environment: String,
    variable: String,
    value: String,
) -> Result<()> {
    core.secret_set(&environment, &variable, &value)
}

#[tauri::command]
pub async fn secret_delete(
    core: State<'_, AppCore>,
    environment: String,
    variable: String,
) -> Result<()> {
    core.secret_delete(&environment, &variable)
}
