// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Secret rescue (`RescueApi`, `rt_app::rescue`): the history scan and the rewrite with `rt_git::rewrite`; the value goes into the keychain, never to the webview.
//!
//! Wave 12, strand G. Commands: `rescue_scan` (progress over a Tauri channel) and
//! `rescue_rewrite`. The functions below take a [`Core`], so `tests/rescue.rs` runs them
//! without a window:
//!
//! - [`scan`]: every commit of `HEAD` and the local branches (`Repo::scan_history`). The
//!   webview gets masked findings only.
//! - [`rewrite`]: refuses with unsaved changes (and, in `rt-git`, with uncommitted ones),
//!   rewrites the local branches with backups first, puts the value into the keychain under
//!   the open collection, the plan's environment and variable, and reopens the collection.
//!   The value passes from the old blob to the keychain on this side only; the result, the
//!   errors and the webview never see it. No push: the UI shows the force-push command.

use rt_app::rescue::{HistoryScan, RescuePlan, RescueResult, ScanProgress};
use rt_app::{AppError, SecretStore};
use rt_git::{Options, Repo};
use tauri::ipc::Channel;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

type Result<T, E = AppError> = std::result::Result<T, E>;

fn repo<S: SecretStore>(core: &Core<S>) -> Result<Repo> {
    let root = core
        .root()
        .ok_or_else(|| AppError::Shell("no collection is open".into()))?;
    Repo::require(&root, &Options::default())
}

/// The history scan of the open collection's repository.
pub fn scan<S: SecretStore>(
    core: &Core<S>,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<HistoryScan> {
    repo(core)?.scan_history(progress)
}

/// Rewrites the history as `plan` says (the user confirmed it in the UI); `now` (Unix
/// seconds) names the backups.
pub fn rewrite<S: SecretStore>(
    core: &Core<S>,
    plan: &RescuePlan,
    now: i64,
) -> Result<RescueResult> {
    if core.has_unsaved_changes() {
        return Err(AppError::Git(
            "save or discard your changes before you rewrite the history".into(),
        ));
    }
    let repo = repo(core)?;
    let result = repo.rewrite_history(plan, now, &mut |value| {
        core.secret_set(&plan.environment, &plan.variable, value)
    })?;
    core.reload()?;
    Ok(result)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppCore) -> Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    use tauri::Manager as _;
    tokio::task::spawn_blocking(move || f(&app.state::<AppCore>()))
        .await
        .map_err(|e| AppError::Shell(format!("secret rescue failed: {e}")))?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn rescue_scan(
    app: tauri::AppHandle,
    on_progress: Channel<ScanProgress>,
) -> Result<HistoryScan, CommandError> {
    blocking(app, move |core| {
        scan(core, &mut |p| {
            // A closed window only stops the progress, not the scan.
            let _ = on_progress.send(p);
        })
    })
    .await
}

#[tauri::command]
pub async fn rescue_rewrite(
    app: tauri::AppHandle,
    plan: RescuePlan,
) -> Result<RescueResult, CommandError> {
    blocking(app, move |core| rewrite(core, &plan, unix_now())).await
}
