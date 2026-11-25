// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! API notebooks for the desktop app (`Backend.notebook`, `rt_app::notebook`, wave 19 strand W).
//! A notebook is a companion file of the active collection, so listing, reading and staging go
//! through the workspace (staged until `save`, like any edit). Running sends the request
//! blocks with the app's services (send guard, cookies, tokens, plugins) and the environment's
//! secret values from the keychain, never the webview's; every run leaves an entry in the audit
//! log (`agents/audit.jsonl`). One run at a time. Outputs are returned, never stored.
//! Commands: `notebook_list`, `notebook_read`, `notebook_stage`, `notebook_run`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rt_app::envcompare::EnvRun;
use rt_app::notebook::{
    Notebook, NotebookEntry, NotebookEnv, NotebookOutcome, NotebookRun, run as run_notebook,
};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// The run in progress, if any.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Marks the run as finished when dropped (also on errors and panics).
struct Busy;

impl Busy {
    fn start() -> Result<Self, AppError> {
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err(AppError::Shell("a notebook is already running".into()));
        }
        Ok(Self)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Runs the blocks of `request.path` on the active collection.
pub async fn run<S: SecretStore>(
    core: &Core<S>,
    request: NotebookRun,
) -> Result<NotebookOutcome, AppError> {
    let _busy = Busy::start()?;
    let notebook = core.with_workspace(|ws, _| rt_app::notebook::read(ws, &request.path))?;
    // Never trusted from the webview, as at send: the keychain is the only source for secrets.
    let input = SendInput {
        environment: request.environment.clone(),
        vars: Vec::new(),
        secrets: Vec::new(),
    };
    let (snapshot, id) = core.run_snapshot(input).await?;
    let parts = Arc::clone(&core.services);
    let data = core.data_dir.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let send_options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        let env_run = EnvRun {
            snapshot,
            services: services.services(),
        };
        let audit = |entry: &rt_app::agents::AuditEntry| {
            if let Some(dir) = &data {
                let _ = rt_app::agents::append_audit(dir, entry);
            }
        };
        let env = NotebookEnv {
            run: &env_run,
            transport: &transport,
            options: &send_options,
            caller: "app",
            audit: &audit,
            cancelled: &|| false,
        };
        runtime.block_on(run_notebook(&env, &notebook, &request))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the notebook run failed: {e}")))?
}

#[tauri::command]
pub async fn notebook_list(core: State<'_, AppCore>) -> Result<Vec<NotebookEntry>, CommandError> {
    Ok(core.with_workspace(|ws, _| Ok(rt_app::notebook::list(ws)))?)
}

#[tauri::command]
pub async fn notebook_read(
    core: State<'_, AppCore>,
    path: String,
) -> Result<Notebook, CommandError> {
    Ok(core.with_workspace(|ws, _| rt_app::notebook::read(ws, &path))?)
}

#[tauri::command]
pub async fn notebook_stage(
    core: State<'_, AppCore>,
    path: String,
    notebook: Notebook,
) -> Result<(), CommandError> {
    Ok(core.with_workspace(|ws, _| rt_app::notebook::stage(ws, &path, &notebook))?)
}

#[tauri::command]
pub async fn notebook_run(
    core: State<'_, AppCore>,
    run: NotebookRun,
) -> Result<NotebookOutcome, CommandError> {
    Ok(self::run(&core, run).await?)
}
