// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Sharing without a server and copying as code (`Backend.share`, `rt_app::share`), answered
//! from the active collection as it is in memory. The base of a link is [`SHARE_BASE`], the
//! address of the public web app; with an empty base the app would copy only the payload as
//! `rt:<payload>` (a bare payload is always understood when pasted). Nothing here makes a network request, and
//! no secret value is read: the keychain is never opened for a link or a snippet.

use rt_app::share::{self, CopiedAs, CopyKind, CopyOptions, ShareLink, ShareOptions, SharePayload};
use rt_app::{AppError, SecretStore};
use tauri::State;

use crate::Core;
use crate::commands::{AppCore, CommandError};

/// The address of the public web app that opens shared links (`<base>#rt=<payload>`), set by
/// the project owner (2026-11-24). No request is ever made to it when a link is made.
pub const SHARE_BASE: &str = "https://app.requesttailor.com/";

/// The base the desktop puts in `ShareOptions.base`: `None` if [`SHARE_BASE`] is empty.
pub fn base() -> Option<&'static str> {
    base_of(SHARE_BASE)
}

fn base_of(constant: &'static str) -> Option<&'static str> {
    (!constant.is_empty()).then_some(constant)
}

/// The link for the request at `path`. The desktop uses its own base, whatever the UI sent.
pub fn link<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    mut options: ShareOptions,
) -> Result<ShareLink, AppError> {
    options.base = base().map(str::to_owned);
    core.with_workspace(|ws, _| share::link(ws, path, &options))
}

pub fn open(input: &str) -> Result<SharePayload, AppError> {
    share::open(input)
}

pub fn copy_as<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    kind: CopyKind,
    options: &CopyOptions,
) -> Result<CopiedAs, AppError> {
    core.with_workspace(|ws, _| share::copy_as(ws, path, kind, options))
}

pub fn qr(link: &str) -> Result<String, AppError> {
    share::qr(link)
}

#[tauri::command]
pub async fn share_link(
    core: State<'_, AppCore>,
    path: String,
    options: ShareOptions,
) -> Result<ShareLink, CommandError> {
    Ok(link(&core, &path, options)?)
}

#[tauri::command]
pub async fn share_open(input: String) -> Result<SharePayload, CommandError> {
    Ok(open(&input)?)
}

#[tauri::command]
pub async fn share_copy_as(
    core: State<'_, AppCore>,
    path: String,
    kind: CopyKind,
    options: CopyOptions,
) -> Result<CopiedAs, CommandError> {
    Ok(copy_as(&core, &path, kind, &options)?)
}

#[tauri::command]
pub async fn share_qr(link: String) -> Result<String, CommandError> {
    Ok(qr(&link)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_is_the_public_web_app() {
        assert_eq!(base(), Some("https://app.requesttailor.com/"));
        assert_eq!(base_of(""), None);
        assert_eq!(
            base_of("https://app.example.test/"),
            Some("https://app.example.test/")
        );
    }
}
