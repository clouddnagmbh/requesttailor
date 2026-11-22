// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Scripts in the desktop app (wave 7, strand A owns this file): trust per collection
//! (`trusted-scripts.json` in the data folder) and the script runner for sends
//! (`rt_script`). Commands: `scripts_trust`, `scripts_set_trusted`.
//!
//! Interface stub: nothing is trusted, sends run no scripts.

use rt_app::scripts::ScriptTrust;
use rt_core::services::ScriptRunner;
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn scripts_trust(core: State<'_, AppCore>) -> Result<ScriptTrust, CommandError> {
    let _ = core;
    Ok(ScriptTrust {
        has_scripts: false,
        trusted: false,
    })
}

#[tauri::command]
pub async fn scripts_set_trusted(
    core: State<'_, AppCore>,
    trusted: bool,
) -> Result<(), CommandError> {
    let _ = (core, trusted);
    Ok(())
}

/// Long-lived script state (trust store, engine).
pub struct ScriptParts;

impl ScriptParts {
    pub fn new(data_dir: Option<&std::path::Path>) -> Self {
        let _ = data_dir;
        Self
    }

    /// The runner for a send from `collection`, `None` if its scripts must not run.
    pub fn for_send(&self, collection: &str) -> ScriptSend<'_> {
        let _ = collection;
        ScriptSend { _parts: self }
    }
}

pub struct ScriptSend<'a> {
    _parts: &'a ScriptParts,
}

impl ScriptSend<'_> {
    pub fn runner(&self) -> Option<&dyn ScriptRunner> {
        None
    }
}
