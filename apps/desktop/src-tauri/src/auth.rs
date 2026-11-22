// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 in the desktop app (wave 5, strand C): tokens in the keychain (`TokenStore`),
//! the system browser plus a loopback listener for the authorization code flow and the
//! `auth-prompt` event for the UI (`AuthPrompt`). Commands: `auth_tokens`,
//! `auth_fetch_token`, `auth_clear_tokens`, `auth_cancel_prompt`.
//!
//! Interface commit: no tokens; fetching answers `unsupported`. Strand C owns this file.

use rt_app::auth::TokenInfo;
use rt_app::{AppError, SendInput};
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn auth_tokens(core: State<'_, AppCore>) -> Result<Vec<TokenInfo>, CommandError> {
    let _ = core;
    Ok(Vec::new())
}

#[tauri::command]
pub async fn auth_fetch_token(
    core: State<'_, AppCore>,
    path: String,
    input: SendInput,
) -> Result<TokenInfo, CommandError> {
    let _ = (core, path, input);
    Err(AppError::Unsupported("OAuth 2.0 is not available yet".into()).into())
}

#[tauri::command]
pub async fn auth_clear_tokens(
    core: State<'_, AppCore>,
    key: Option<String>,
) -> Result<(), CommandError> {
    let _ = (core, key);
    Ok(())
}

#[tauri::command]
pub async fn auth_cancel_prompt(core: State<'_, AppCore>) -> Result<(), CommandError> {
    let _ = core;
    Ok(())
}

/// Token storage (keychain) and the prompt channel to the UI, kept by `Core`.
pub struct AuthParts {
    _private: (),
}

impl AuthParts {
    pub fn new(data_dir: Option<&std::path::Path>) -> Self {
        let _ = data_dir;
        Self { _private: () }
    }

    pub fn for_send(&self, collection: &str) -> AuthSend<'_> {
        let _ = collection;
        AuthSend { _parts: self }
    }
}

/// Token cache and prompts of one send.
pub struct AuthSend<'a> {
    _parts: &'a AuthParts,
}

impl AuthSend<'_> {
    pub fn tokens(&self) -> Option<&dyn rt_core::services::TokenStore> {
        None
    }

    pub fn prompt(&self) -> Option<&dyn rt_core::services::AuthPrompt> {
        None
    }
}
