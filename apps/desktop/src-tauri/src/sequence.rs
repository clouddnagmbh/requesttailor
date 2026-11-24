// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sequence diagrams for the desktop app (`Backend.sequence`, `rt_app::sequence`). Wave 19 interface:
//! registered, "not implemented yet"; strand G answers them.

use rt_app::AppError;

use crate::commands::CommandError;

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported(
        "Sequence diagrams is not implemented yet (wave 19, strand G).".into(),
    )
    .into())
}

#[tauri::command]
pub async fn sequence_model(
    _app: tauri::AppHandle,
    _source: rt_app::sequence::SequenceSource,
    _options: rt_app::sequence::SequenceOptions,
) -> Result<rt_app::sequence::SequenceModel, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn sequence_render(
    _app: tauri::AppHandle,
    _model: rt_app::sequence::SequenceModel,
    _options: rt_app::sequence::SequenceOptions,
) -> Result<rt_app::sequence::SequenceText, CommandError> {
    not_yet()
}

#[tauri::command]
pub async fn sequence_clear(_app: tauri::AppHandle) -> Result<(), CommandError> {
    not_yet()
}
