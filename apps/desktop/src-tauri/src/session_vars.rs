// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Session variables of the desktop app (wave 7, strand C): what scripts, actions and "set as
//! variable" set, in memory per collection and environment (`rt_app::scripts::SessionVars`).
//! Commands: `scripts_session_vars`, `scripts_set_session_var`, `scripts_clear_session_vars`.
//!
//! Every send and preview gets the session's variables of the selected environment in its
//! runtime layer, before the caller's own `vars` (which win), so provenance shows them as
//! runtime variables. After a send, the report's `var_changes` are kept for the session,
//! whatever their scope (`environment`, `collection`, `global` changes live in memory too;
//! files are never written). Values are as reported, so a captured secret stays masked.

use std::collections::HashMap;
use std::sync::Mutex;

use rt_app::scripts::SessionVars;
use rt_app::{SecretStore, SendInput};
use rt_core::script::VarChange;
use rt_core::send::SendReport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

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

    /// `input` with the session's variables of its environment in front of its own `vars`.
    pub fn input_for(&self, collection: &str, mut input: SendInput) -> SendInput {
        let environment = input.environment.clone().unwrap_or_default();
        let mut vars = self.with(collection, |s| s.runtime(&environment));
        vars.append(&mut input.vars);
        input.vars = vars;
        input
    }

    /// Keeps what a send set (`report.var_changes`) for its environment.
    pub fn keep(&self, collection: &str, report: &SendReport) {
        if report.var_changes.is_empty() {
            return;
        }
        let environment = report.environment.clone().unwrap_or_default();
        self.with(collection, |s| {
            for change in &report.var_changes {
                s.apply(&environment, change);
            }
        });
    }
}

impl<S: SecretStore> Core<S> {
    /// The session's variables for `environment` (`""` for none) of the open collection.
    pub fn session_vars(&self, environment: &str) -> Vec<VarChange> {
        match self.collection_id() {
            Some(id) => self.services.session.with(&id, |s| s.list(environment)),
            None => Vec::new(),
        }
    }

    pub fn set_session_var(&self, environment: &str, change: &VarChange) {
        if let Some(id) = self.collection_id() {
            self.services
                .session
                .with(&id, |s| s.apply(environment, change));
        }
    }

    pub fn clear_session_vars(&self, environment: &str) {
        if let Some(id) = self.collection_id() {
            self.services.session.with(&id, |s| s.clear(environment));
        }
    }
}

#[tauri::command]
pub async fn scripts_session_vars(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<Vec<VarChange>, CommandError> {
    Ok(core.session_vars(&environment))
}

#[tauri::command]
pub async fn scripts_set_session_var(
    core: State<'_, AppCore>,
    environment: String,
    change: VarChange,
) -> Result<(), CommandError> {
    core.set_session_var(&environment, &change);
    Ok(())
}

#[tauri::command]
pub async fn scripts_clear_session_vars(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<(), CommandError> {
    core.clear_session_vars(&environment);
    Ok(())
}
