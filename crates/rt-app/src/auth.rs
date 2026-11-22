// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! OAuth 2.0 in the app (wave 5, strand C): the cached tokens as the UI lists them (never
//! their values) and the prompts the UI shows while a flow runs.
//!
//! Tokens are secrets: natively in the OS keychain, in the web in the encrypted browser
//! store, under `SecretKey { collection, environment: "oauth2", variable: <cache key> }`.
//! Changes here are interface changes.

use serde::{Deserialize, Serialize};

pub use rt_core::services::DeviceCodePrompt;

/// A cached token, without its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenInfo {
    /// `rt_core::auth::oauth2::cache_key`.
    pub key: String,
    /// Token endpoint host, client id and scope, for display.
    pub label: String,
    /// Unix milliseconds.
    pub obtained_at: i64,
    pub expires_at: Option<i64>,
    pub refreshable: bool,
}

/// What the UI shows while a flow waits for the user. Pushed by the shell
/// (`Backend.auth.onPrompt`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AuthPrompt {
    /// The system browser (desktop, CLI) or a popup (web) shows the provider's login.
    Browser {
        url: String,
    },
    DeviceCode(DeviceCodePrompt),
    /// The flow ended (token received, failed or cancelled): close the prompt.
    Done,
}
