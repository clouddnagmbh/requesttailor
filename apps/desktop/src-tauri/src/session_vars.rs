// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Session variables of the desktop app (wave 7, strand C owns this file): what scripts,
//! actions and "set as variable" set, in memory per collection and environment
//! (`rt_app::scripts::SessionVars`), added to the runtime layer of later sends. Commands:
//! `scripts_session_vars`, `scripts_set_session_var`, `scripts_clear_session_vars`.
//!
//! Interface stub: kept, not yet used by sends.

use std::collections::HashMap;
use std::sync::Mutex;

use rt_app::scripts::SessionVars;
use rt_core::script::VarChange;
use tauri::State;

use crate::commands::{AppCore, CommandError};

/// Session variables per collection id.
#[derive(Default)]
pub struct SessionParts {
    by_collection: Mutex<HashMap<String, SessionVars>>,
}

impl SessionParts {
    pub fn with<T>(&self, collection: &str, f: impl FnOnce(&mut SessionVars) -> T) -> T {
        let mut map = self
            .by_collection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(map.entry(collection.to_owned()).or_default())
    }
}

#[tauri::command]
pub async fn scripts_session_vars(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<Vec<VarChange>, CommandError> {
    let Some(id) = core.collection_id() else {
        return Ok(Vec::new());
    };
    Ok(core.services.session.with(&id, |s| s.list(&environment)))
}

#[tauri::command]
pub async fn scripts_set_session_var(
    core: State<'_, AppCore>,
    environment: String,
    change: VarChange,
) -> Result<(), CommandError> {
    if let Some(id) = core.collection_id() {
        core.services
            .session
            .with(&id, |s| s.apply(&environment, &change));
    }
    Ok(())
}

#[tauri::command]
pub async fn scripts_clear_session_vars(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<(), CommandError> {
    if let Some(id) = core.collection_id() {
        core.services.session.with(&id, |s| s.clear(&environment));
    }
    Ok(())
}
