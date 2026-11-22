// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The cookie jar of the desktop app (wave 5, strand D): one jar per collection and
//! environment, persisted in the data folder, passed to `send_with` as `CookieJar`. Commands:
//! `cookie_list`, `cookie_delete`, `cookie_clear`.
//!
//! Interface commit: an empty jar. Strand D owns this file.

use rt_app::cookies::CookieInfo;
use tauri::State;

use crate::commands::{AppCore, CommandError};

#[tauri::command]
pub async fn cookie_list(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<Vec<CookieInfo>, CommandError> {
    let _ = (core, environment);
    Ok(Vec::new())
}

#[tauri::command]
pub async fn cookie_delete(
    core: State<'_, AppCore>,
    environment: String,
    domain: String,
    path: String,
    name: String,
) -> Result<(), CommandError> {
    let _ = (core, environment, domain, path, name);
    Ok(())
}

#[tauri::command]
pub async fn cookie_clear(
    core: State<'_, AppCore>,
    environment: String,
) -> Result<(), CommandError> {
    let _ = (core, environment);
    Ok(())
}

/// The cookie jars of all collections, kept by `Core`.
pub struct CookieParts {
    _private: (),
}

impl CookieParts {
    pub fn new(data_dir: Option<&std::path::Path>) -> Self {
        let _ = data_dir;
        Self { _private: () }
    }

    pub fn for_send(&self, collection: &str) -> CookieSend<'_> {
        let _ = collection;
        CookieSend { _parts: self }
    }
}

/// The jar of one send (the scope inside it is the environment).
pub struct CookieSend<'a> {
    _parts: &'a CookieParts,
}

impl CookieSend<'_> {
    pub fn jar(&self) -> Option<&dyn rt_core::services::CookieJar> {
        None
    }
}
