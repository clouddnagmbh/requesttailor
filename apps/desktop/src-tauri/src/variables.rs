// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Variable completion (wave 14): `variables_scope` lists every variable a request, folder or
//! the collection sees (`rt_app::variables::scope`, secret values never leave Rust),
//! `variables_define` adds a plain variable to the collection or a folder (unsaved).

use rt_app::SendInput;
use rt_app::variables::VariableScope;
use rt_core::model::Variable;
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn variables_scope(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<VariableScope, CommandError> {
    Ok(core.variables(&path, input)?)
}

#[tauri::command]
pub async fn variables_define(
    core: State<'_, AppCore>,
    folder: String,
    variable: Variable,
) -> Result<(), CommandError> {
    Ok(core.with_workspace(|ws, _| ws.define_variable(&folder, &variable))?)
}
