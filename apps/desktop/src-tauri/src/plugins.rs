// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Plugins in the desktop app (wave 5, strand A): installed modules live in the data folder's
//! `plugins/` directory (shared with the CLI), a `rt_plugin::host::PluginHost` runs them, and
//! `Core::send` passes it to `send_with` as `RequestHooks`. Commands: `plugin_list`,
//! `plugin_inspect`, `plugin_install` (raw body: the `.wasm` bytes), `plugin_remove`,
//! `plugin_set_enabled`, `plugin_render_tab`, `plugin_action`, `import_analyze_plugin` (raw
//! body as `import_analyze`, header with `plugin` and `importer`).
//!
//! Interface commit: every command answers `unsupported`. Strand A owns this file.

use rt_app::AppError;
use rt_app::ImportPlan;
use rt_app::plugins::{Manifest, PluginInfo, TabTarget, ViewReply};
use tauri::State;
use tauri::ipc::Request;

use crate::commands::{AppCore, CommandError};

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported("plugins are not available yet".into()).into())
}

#[tauri::command]
pub async fn plugin_list(core: State<'_, AppCore>) -> Result<Vec<PluginInfo>, CommandError> {
    let _ = core;
    Ok(Vec::new())
}

#[tauri::command]
pub async fn plugin_inspect(request: Request<'_>) -> Result<Manifest, CommandError> {
    let _ = request;
    not_yet()
}

#[tauri::command]
pub async fn plugin_install(
    core: State<'_, AppCore>,
    request: Request<'_>,
) -> Result<PluginInfo, CommandError> {
    let _ = (core, request);
    not_yet()
}

#[tauri::command]
pub async fn plugin_remove(core: State<'_, AppCore>, id: String) -> Result<(), CommandError> {
    let _ = (core, id);
    not_yet()
}

#[tauri::command]
pub async fn plugin_set_enabled(
    core: State<'_, AppCore>,
    id: String,
    enabled: bool,
) -> Result<(), CommandError> {
    let _ = (core, id, enabled);
    not_yet()
}

#[tauri::command]
pub async fn plugin_render_tab(
    core: State<'_, AppCore>,
    plugin: String,
    tab: String,
    target: TabTarget,
    state: Option<serde_json::Value>,
) -> Result<ViewReply, CommandError> {
    let _ = (core, plugin, tab, target, state);
    not_yet()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn plugin_action(
    core: State<'_, AppCore>,
    plugin: String,
    tab: String,
    action: String,
    target: TabTarget,
    state: Option<serde_json::Value>,
    input: Vec<(String, String)>,
) -> Result<ViewReply, CommandError> {
    let _ = (core, plugin, tab, action, target, state, input);
    not_yet()
}

#[tauri::command]
pub async fn import_analyze_plugin(
    core: State<'_, AppCore>,
    request: Request<'_>,
) -> Result<ImportPlan, CommandError> {
    let _ = (core, request);
    not_yet()
}

/// Installed plugins and their host, kept by `Core` (`services::ServiceParts`).
pub struct PluginParts {
    _private: (),
}

impl PluginParts {
    pub fn new(data_dir: Option<&std::path::Path>) -> Self {
        let _ = data_dir;
        Self { _private: () }
    }

    pub fn for_send(&self, collection: &str, environment: Option<&str>) -> PluginSend<'_> {
        let _ = (collection, environment);
        PluginSend { _parts: self }
    }
}

/// The plugins of one send.
pub struct PluginSend<'a> {
    _parts: &'a PluginParts,
}

impl PluginSend<'_> {
    pub fn hooks(&self) -> Option<&dyn rt_core::services::RequestHooks> {
        None
    }
}
