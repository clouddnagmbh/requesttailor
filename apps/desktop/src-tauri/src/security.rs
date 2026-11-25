// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Passive security checks for the desktop app (`Backend.security`, `rt_app::security`).
//! Answered from the kept responses on the Rust side: nothing is sent. The device's settings
//! (one flag and rule ids, never a value) are `security.json` in the data folder, shared with
//! `rt send --checks`; a collection switches rules off in `checks.ignore` of
//! `opencollection.yml`.

use rt_app::inspect;
use rt_app::security::{self, SecurityFinding, SecurityRule, SecuritySettings, SecurityTarget};
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::inspect::device_ignored;

#[tauri::command]
pub async fn security_check(
    core: State<'_, AppCore>,
    target: SecurityTarget,
) -> Result<Vec<SecurityFinding>, CommandError> {
    let device = device_ignored(core.data_dir.as_deref());
    let settings = security::read_settings(core.data_dir.as_deref());
    Ok(core.with_responses(|ws, responses| {
        let ignore = inspect::all_ignored(ws, &device);
        security::passive_of(responses, u64::from(target.response_id), &ignore, &settings)
    })?)
}

#[tauri::command]
pub async fn security_rules() -> Result<Vec<SecurityRule>, CommandError> {
    Ok(security::rules())
}

#[tauri::command]
pub async fn security_settings(core: State<'_, AppCore>) -> Result<SecuritySettings, CommandError> {
    Ok(security::read_settings(core.data_dir.as_deref()))
}

#[tauri::command]
pub async fn security_set_settings(
    core: State<'_, AppCore>,
    settings: SecuritySettings,
) -> Result<(), CommandError> {
    Ok(security::write_settings(
        core.data_dir.as_deref(),
        &settings,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_are_listed_with_their_categories() {
        let rules = security::rules();
        assert!(
            rules
                .iter()
                .any(|r| r.id == "sec/tls-weak-key" && r.native_only)
        );
        assert!(rules.iter().any(|r| r.id == "chk/hsts-missing"));
    }
}
