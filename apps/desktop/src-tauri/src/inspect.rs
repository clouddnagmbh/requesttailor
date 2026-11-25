// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Tokens and passive response checks for the desktop app (`Backend.inspect`,
//! `rt_app::inspect`). Answered from the model and the kept responses on the Rust side; nothing
//! is sent and nothing is written but the list of rules switched off on this device
//! (`checks-ignored.json` in the data folder; rule ids only) or, for the collection, the
//! staged `opencollection.yml`. A token value goes back to the webview only for the view that
//! asked for it (`inspect_jwt`, `inspect_tokens`).

use std::path::{Path, PathBuf};

use rt_app::inspect::{self, CheckScope, FoundToken, InspectTarget, JwtInfo, Observation};
use rt_app::{AppError, SendInput};
use tauri::State;

use crate::commands::{AppCore, CommandError};

const DEVICE_FILE: &str = "checks-ignored.json";

fn device_path(data_dir: Option<&Path>) -> Option<PathBuf> {
    data_dir.map(|d| d.join(DEVICE_FILE))
}

/// The rule ids switched off on this device (empty if there is no file or it is odd).
pub fn device_ignored(data_dir: Option<&Path>) -> Vec<String> {
    device_path(data_dir)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default()
}

/// Switches `id` off on this device.
pub fn ignore_on_device(data_dir: Option<&Path>, id: &str) -> Result<(), AppError> {
    if !inspect::all_rules().iter().any(|r| r.id == id) {
        return Err(AppError::Shell(format!("unknown check: {id}")));
    }
    let Some(path) = device_path(data_dir) else {
        return Err(AppError::Shell("this build has no data folder".into()));
    };
    let mut ids = device_ignored(data_dir);
    if ids.iter().any(|i| i == id) {
        return Ok(());
    }
    ids.push(id.to_owned());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Shell(e.to_string()))?;
    }
    let text = serde_json::to_string_pretty(&ids).map_err(|e| AppError::Shell(e.to_string()))?;
    std::fs::write(path, text).map_err(|e| AppError::Shell(e.to_string()))
}

#[tauri::command]
pub async fn inspect_jwt(token: String) -> Result<JwtInfo, CommandError> {
    Ok(inspect::jwt(&token)?)
}

#[tauri::command]
pub async fn inspect_tokens(
    core: State<'_, AppCore>,
    target: InspectTarget,
) -> Result<Vec<FoundToken>, CommandError> {
    // Secret values are never taken from the webview: the core reads them as for a send.
    let input = SendInput {
        environment: target.environment.clone(),
        vars: Vec::new(),
        secrets: Vec::new(),
    };
    let path = target.path.clone();
    let mut found = core
        .with_send_input(input, |ws, input, _| {
            inspect::request_tokens(ws, &path, input)
        })
        .await?;
    if let Some(id) = target.response_id {
        found.extend(core.with_responses(|_, responses| inspect::response_tokens(responses, id))?);
    }
    Ok(found)
}

#[tauri::command]
pub async fn inspect_checks(
    core: State<'_, AppCore>,
    target: InspectTarget,
) -> Result<Vec<Observation>, CommandError> {
    let Some(id) = target.response_id else {
        return Ok(Vec::new());
    };
    let mut device = device_ignored(core.data_dir.as_deref());
    // Wave 19: the device's security settings switch the security rules off (as a group or one
    // by one) in the same pass.
    device.extend(rt_app::security::ignored_by(
        &rt_app::security::read_settings(core.data_dir.as_deref()),
    ));
    Ok(core.with_responses(|ws, responses| {
        let ignore = inspect::all_ignored(ws, &device);
        inspect::checks_of(responses, id, &ignore)
    })?)
}

#[tauri::command]
pub async fn inspect_ignore_check(
    core: State<'_, AppCore>,
    id: String,
    scope: CheckScope,
) -> Result<(), CommandError> {
    match scope {
        CheckScope::Device => Ok(ignore_on_device(core.data_dir.as_deref(), &id)?),
        CheckScope::Collection => {
            Ok(core.with_workspace(|ws, _| inspect::ignore_in_collection(ws, &id))?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_list_is_a_file_of_rule_ids() {
        let dir = std::env::temp_dir().join(format!("rt-inspect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(device_ignored(Some(&dir)).is_empty());
        ignore_on_device(Some(&dir), "chk/hsts-missing").unwrap();
        ignore_on_device(Some(&dir), "chk/hsts-missing").unwrap();
        ignore_on_device(Some(&dir), "chk/stack-trace").unwrap();
        assert_eq!(
            device_ignored(Some(&dir)),
            ["chk/hsts-missing", "chk/stack-trace"]
        );
        assert!(ignore_on_device(Some(&dir), "chk/nope").is_err());
        assert!(ignore_on_device(None, "chk/stack-trace").is_err());
        std::fs::write(dir.join(DEVICE_FILE), "not json").unwrap();
        assert!(device_ignored(Some(&dir)).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
