// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sharing without a server and copying as code (`Backend.share`, `rt_app::share`). Wave 17
//! interface: registered, "not implemented yet"; strand C answers them. The base of a link is
//! [`SHARE_BASE`]: empty until the project owner sets the address of the public web app, and
//! with an empty base the app copies only the payload as `rt:<payload>`.

use rt_app::AppError;
use rt_app::share::{CopiedAs, CopyKind, CopyOptions, ShareLink, ShareOptions, SharePayload};

use crate::commands::CommandError;

/// The address of the public web app that opens shared links (`<base>#rt=<payload>`). Empty
/// until the project owner decides; no request is ever made to it when a link is made.
pub const SHARE_BASE: &str = "";

fn not_yet<T>() -> Result<T, CommandError> {
    Err(AppError::Unsupported("Sharing is not implemented yet (wave 17, strand C).".into()).into())
}

/// The base the desktop puts in `ShareOptions.base`: `None` while [`SHARE_BASE`] is empty.
pub fn base() -> Option<&'static str> {
    (!SHARE_BASE.is_empty()).then_some(SHARE_BASE)
}

#[tauri::command]
pub async fn share_link(
    _app: tauri::AppHandle,
    path: String,
    options: ShareOptions,
) -> Result<ShareLink, CommandError> {
    let _ = (path, options);
    not_yet()
}

#[tauri::command]
pub async fn share_open(
    _app: tauri::AppHandle,
    input: String,
) -> Result<SharePayload, CommandError> {
    let _ = input;
    not_yet()
}

#[tauri::command]
pub async fn share_copy_as(
    _app: tauri::AppHandle,
    path: String,
    kind: CopyKind,
    options: CopyOptions,
) -> Result<CopiedAs, CommandError> {
    let _ = (path, kind, options);
    not_yet()
}

#[tauri::command]
pub async fn share_qr(_app: tauri::AppHandle, link: String) -> Result<String, CommandError> {
    let _ = link;
    not_yet()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_base_is_empty_until_the_owner_sets_it() {
        assert_eq!(super::base(), None);
    }
}
