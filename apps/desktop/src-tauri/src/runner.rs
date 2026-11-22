// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The collection runner in the desktop app (wave 7, strand D owns this file): runs
//! `rt_core::runner::run` on a blocking thread with the send services, progress over a Tauri
//! channel. Commands: `runner_plan`, `runner_parse_data`, `runner_run`, `runner_cancel`.
//!
//! Interface stub: an empty plan and an empty run.

use rt_core::runner::{RunEvent, RunPlan, RunSummary};
use tauri::State;
use tauri::ipc::Channel;

use crate::commands::{AppCore, CommandError};

/// A data row as the UI gets it.
pub type DataRow = Vec<(String, serde_json::Value)>;

#[tauri::command]
pub async fn runner_plan(
    core: State<'_, AppCore>,
    folder: Option<String>,
) -> Result<Vec<String>, CommandError> {
    let _ = (core, folder);
    Ok(Vec::new())
}

#[tauri::command]
pub async fn runner_parse_data(name: String, data: Vec<u8>) -> Result<Vec<DataRow>, CommandError> {
    let _ = (name, data);
    Ok(Vec::new())
}

#[tauri::command]
pub async fn runner_run(
    core: State<'_, AppCore>,
    plan: RunPlan,
    input: rt_app::SendInput,
    on_event: Channel<RunEvent>,
) -> Result<RunSummary, CommandError> {
    let _ = (core, plan, input);
    let summary = RunSummary::default();
    let _ = on_event.send(RunEvent::Started { total: 0 });
    let _ = on_event.send(RunEvent::Finished {
        summary: summary.clone(),
    });
    Ok(summary)
}

#[tauri::command]
pub async fn runner_cancel(core: State<'_, AppCore>) -> Result<(), CommandError> {
    let _ = core;
    Ok(())
}
