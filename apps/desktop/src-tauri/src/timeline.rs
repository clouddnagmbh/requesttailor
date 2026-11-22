// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Time travel per request (`TimelineApi`, `rt_app::timeline`): file history with `rt-git`
//! (`Repo::file_log`), blame per field, old versions, and the test results recorded per commit
//! in the data folder (`rt_app::timeline::results_file`, never in the repository).
//!
//! Wave 12, strand D. Interface placeholder: the commands are registered and refuse until the
//! strand implements them here; [`RunRecorder`] is already called by every run of the runner
//! (`runner.rs`) and records nothing yet.

use rt_app::{AppError, SecretStore};
use rt_core::runner::RunEvent;

use crate::commands::CommandError;
use crate::core::Core;

fn not_yet() -> CommandError {
    AppError::Unsupported("The request history arrives in wave 12 (strand D).".into()).into()
}

/// Watches one run of the open collection and records its results under `HEAD` when the run
/// used exactly `HEAD`'s files (see `rt_app::timeline`). Created before the run starts.
pub struct RunRecorder {}

impl RunRecorder {
    pub fn for_run<S: SecretStore>(core: &Core<S>) -> Self {
        let _ = core;
        Self {}
    }

    /// Every event of the run, as the runner emits it.
    pub fn observe(&self, event: &RunEvent) {
        let _ = event;
    }
}

#[tauri::command]
pub async fn timeline_history() -> Result<rt_app::timeline::RequestTimeline, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn timeline_blame() -> Result<Vec<rt_app::timeline::FieldBlame>, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn timeline_version() -> Result<rt_core::model::RequestSpec, CommandError> {
    Err(not_yet())
}

#[tauri::command]
pub async fn timeline_results() -> Result<Vec<rt_app::timeline::TestRecord>, CommandError> {
    Err(not_yet())
}
