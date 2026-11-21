// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Tauri commands: one per `Backend` method (`apps/ui/src/commands/backend.ts`), forwarding to
//! [`Core`]. Arguments arrive camelCase from JavaScript (`requestPath` → `request_path`); the
//! TypeScript side is `apps/desktop/src/backend.ts`. Errors are [`CommandError`]
//! (`{ kind, message }`: the `AppError` kind and its readable message, as in the web shell),
//! which the TypeScript side rethrows as `BackendError`.
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
type Result<T> = std::result::Result<T, CommandError>;

/// What a failed command rejects with in JavaScript.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CommandError {
    /// The serialized `AppError` kind, e.g. `request-not-found`.
    pub kind: String,
    /// Readable message, e.g. `request not found: users/get.yml`.
    pub message: String,
}

impl From<AppError> for CommandError {
    fn from(e: AppError) -> Self {
        let kind = serde_json::to_value(&e)
            .ok()
            .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
            .unwrap_or_else(|| "shell".to_owned());
        Self {
            kind,
            message: e.to_string(),
        }
    }
}

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
    Ok(Some(core.open(&path)?))
}

#[tauri::command]
pub async fn tree(core: State<'_, AppCore>) -> Result<TreeNode> {
    Ok(core.tree()?)
}

#[tauri::command]
pub async fn get_request(core: State<'_, AppCore>, path: String) -> Result<RequestSpec> {
    Ok(core.get_request(&path)?)
}

#[tauri::command]
pub async fn create_request(
    core: State<'_, AppCore>,
    folder: String,
    name: String,
) -> Result<String> {
    Ok(core.create_request(&folder, &name)?)
}

#[tauri::command]
pub async fn update_request(
    core: State<'_, AppCore>,
    path: String,
    spec: RequestSpec,
) -> Result<()> {
    Ok(core.update_request(&path, spec)?)
}

#[tauri::command]
pub async fn dirty(core: State<'_, AppCore>) -> Result<Vec<String>> {
    Ok(core.dirty()?)
}

#[tauri::command]
pub async fn save(core: State<'_, AppCore>) -> Result<Vec<FileChange>> {
    Ok(core.save()?)
}

#[tauri::command]
pub async fn environments(core: State<'_, AppCore>) -> Result<Vec<Environment>> {
    Ok(core.environments()?)
}

#[tauri::command]
pub async fn update_environment(core: State<'_, AppCore>, env: Environment) -> Result<()> {
    Ok(core.update_environment(env)?)
}

#[tauri::command]
pub async fn delete_environment(core: State<'_, AppCore>, name: String) -> Result<()> {
    Ok(core.delete_environment(&name)?)
}

#[tauri::command]
pub async fn preview(core: State<'_, AppCore>, path: String, input: SendInput) -> Result<Preview> {
    Ok(core.preview(&path, input)?)
}

#[tauri::command]
pub async fn send(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<StoredResponse> {
    Ok(core.send(&path, input).await?)
}

#[tauri::command]
pub async fn response_lines(
    core: State<'_, AppCore>,
    id: u64,
    start: u64,
    count: u32,
) -> Result<LineSlice> {
    Ok(core.response_lines(id, start, count)?)
}

/// Raw bytes, sent as binary IPC (an `ArrayBuffer` in JavaScript), not as a JSON array.
#[tauri::command]
pub async fn response_bytes(
    core: State<'_, AppCore>,
    id: u64,
    offset: u64,
    length: u32,
) -> Result<tauri::ipc::Response> {
    Ok(tauri::ipc::Response::new(
        core.response_bytes(id, offset, length)?,
    ))
}

#[tauri::command]
pub async fn filter_json(core: State<'_, AppCore>, id: u64, expression: String) -> Result<String> {
    Ok(core.filter_json(id, &expression)?)
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
    Ok(core.secret_has(&environment, &variable)?)
}

#[tauri::command]
pub async fn secret_get(
    core: State<'_, AppCore>,
    environment: String,
    variable: String,
) -> Result<Option<String>> {
    Ok(core.secret_get(&environment, &variable)?)
}

#[tauri::command]
pub async fn secret_set(
    core: State<'_, AppCore>,
    environment: String,
    variable: String,
    value: String,
) -> Result<()> {
    Ok(core.secret_set(&environment, &variable, &value)?)
}

#[tauri::command]
pub async fn secret_delete(
    core: State<'_, AppCore>,
    environment: String,
    variable: String,
) -> Result<()> {
    Ok(core.secret_delete(&environment, &variable)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_errors_carry_kind_and_readable_message() {
        let e = CommandError::from(AppError::RequestNotFound("a.yml".into()));
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            serde_json::json!({ "kind": "request-not-found", "message": "request not found: a.yml" })
        );
        let e = CommandError::from(AppError::ResponseNotFound(7));
        assert_eq!(e.kind, "response-not-found");
        assert_eq!(e.message, "response not found: 7");
    }
}
