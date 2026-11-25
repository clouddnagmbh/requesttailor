// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The webhook inbox for the desktop app (`Backend.inbox`, `rt_app::inbox`, wave 18, strand E).
//! One inbox per process: it listens on loopback only after `inbox_start`, never at app start,
//! and holds what it captured in memory only. The UI polls `inbox_events`.

use std::sync::OnceLock;

use rt_app::AppError;
use rt_app::inbox::{CapturedRequest, Inbox, InboxDraft, InboxPage, InboxSettings, InboxStatus};

use crate::commands::CommandError;

fn inbox() -> &'static Inbox {
    static INBOX: OnceLock<Inbox> = OnceLock::new();
    INBOX.get_or_init(Inbox::new)
}

/// Blocking calls (a stop joins the listener thread) run off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce(&Inbox) -> Result<T, AppError> + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(move || f(inbox()))
        .await
        .map_err(|e| AppError::Shell(e.to_string()))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn inbox_settings(_app: tauri::AppHandle) -> Result<InboxSettings, CommandError> {
    Ok(inbox().settings())
}

#[tauri::command]
pub async fn inbox_set_settings(
    _app: tauri::AppHandle,
    settings: InboxSettings,
) -> Result<(), CommandError> {
    inbox().set_settings(settings).map_err(Into::into)
}

#[tauri::command]
pub async fn inbox_start(_app: tauri::AppHandle) -> Result<InboxStatus, CommandError> {
    blocking(Inbox::start).await
}

#[tauri::command]
pub async fn inbox_stop(_app: tauri::AppHandle) -> Result<InboxStatus, CommandError> {
    blocking(|i| Ok(i.stop())).await
}

#[tauri::command]
pub async fn inbox_status(_app: tauri::AppHandle) -> Result<InboxStatus, CommandError> {
    Ok(inbox().status())
}

#[tauri::command]
pub async fn inbox_events(_app: tauri::AppHandle, since: u64) -> Result<InboxPage, CommandError> {
    Ok(inbox().events(since))
}

#[tauri::command]
pub async fn inbox_get(
    _app: tauri::AppHandle,
    id: u64,
) -> Result<Option<CapturedRequest>, CommandError> {
    Ok(inbox().get(id))
}

#[tauri::command]
pub async fn inbox_clear(_app: tauri::AppHandle) -> Result<(), CommandError> {
    inbox().clear();
    Ok(())
}

#[tauri::command]
pub async fn inbox_make_request(
    _app: tauri::AppHandle,
    id: u64,
) -> Result<InboxDraft, CommandError> {
    inbox().make_request(id).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inbox_does_not_listen_until_started() {
        let status = inbox().status();
        assert!(!status.running);
        assert_eq!(status.port, None);
    }
}
