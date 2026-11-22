// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Full-text search over the open collection (`rt_app::search`). Owner: wave 4, strand E.

use rt_app::SearchHit;
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn search(
    core: State<'_, AppCore>,
    query: String,
    limit: u32,
) -> Result<Vec<SearchHit>, CommandError> {
    Ok(core.search(&query, limit as usize)?)
}
