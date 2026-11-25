// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sequence diagrams for the desktop app (`Backend.sequence`, `rt_app::sequence`, wave 19
//! strand G). The core's one [`Recorder`](rt_app::sequence::Recorder) is an exchange observer
//! of every send (`ServiceParts.recorder`, added to the observers when the core is built); the
//! runner marks the start and end of a run. Sources: `run`, `session` and `pages` come from
//! the recorder, `mcp` from the audit log (`agents/audit.jsonl`), `history` from the history
//! of the active collection. Commands: `sequence_model`, `sequence_render`, `sequence_clear`.
//! Everything shown is masked; values only with `include_values`.

use rt_app::sequence::{
    SequenceModel, SequenceOptions, SequenceSource, SequenceText, model_from_audit,
    model_from_history,
};
use rt_app::{AppError, SecretStore};
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

/// Most audit entries one diagram reads.
const AUDIT_LIMIT: usize = 500;
/// Most history entries one diagram reads.
const HISTORY_LIMIT: usize = 500;

/// The model of `source`.
pub fn model<S: SecretStore>(
    core: &Core<S>,
    source: &SequenceSource,
    options: &SequenceOptions,
) -> Result<SequenceModel, AppError> {
    match source {
        SequenceSource::Mcp { since } => {
            let Some(dir) = core.data_dir.as_deref() else {
                return Ok(model_from_audit(&[], options));
            };
            let mut entries = rt_app::agents::read_audit(dir, AUDIT_LIMIT)
                .map_err(|e| AppError::Shell(format!("cannot read the audit log: {e}")))?;
            if let Some(since) = since {
                entries.retain(|e| e.at >= *since);
            }
            Ok(model_from_audit(&entries, options))
        }
        SequenceSource::History { ids } => {
            let Some(id) = core.collection_id() else {
                return Ok(model_from_history(&[], options));
            };
            let mut entries = core.history.list(&id, "", HISTORY_LIMIT);
            entries.retain(|e| ids.is_empty() || ids.contains(&(e.id as u32)));
            // Oldest first, the way they happened.
            entries.reverse();
            Ok(model_from_history(&entries, options))
        }
        other => core
            .services
            .recorder
            .model(other, options)
            .map_err(AppError::Shell),
    }
}

#[tauri::command]
pub async fn sequence_model(
    core: State<'_, AppCore>,
    source: SequenceSource,
    options: SequenceOptions,
) -> Result<SequenceModel, CommandError> {
    Ok(model(&core, &source, &options)?)
}

#[tauri::command]
pub async fn sequence_render(
    model: SequenceModel,
    options: SequenceOptions,
) -> Result<SequenceText, CommandError> {
    Ok(rt_app::sequence::render(&model, &options))
}

#[tauri::command]
pub async fn sequence_clear(core: State<'_, AppCore>) -> Result<(), CommandError> {
    core.services.recorder.clear();
    Ok(())
}
