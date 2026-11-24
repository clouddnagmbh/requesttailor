// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Comparing environments for the desktop app (`Backend.envCompare`, `rt_app::envcompare`):
//! the reading requests of the active collection run in two or more environments, one request
//! at a time, with the app's services and each environment's secret values (keychain and
//! external secrets, never the webview's). Command: `envcompare_run`. One comparison at a
//! time. Nothing is stored; variable changes stay in the run.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rt_app::envcompare::{EnvCompareRequest, EnvComparison, EnvRun, compare_environments};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// The comparison in progress, if any.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Marks the comparison as finished when dropped (also on errors and panics).
struct Busy;

impl Busy {
    fn start() -> Result<Self, AppError> {
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err(AppError::Shell(
                "an environment comparison is already in progress".into(),
            ));
        }
        Ok(Self)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Runs `request` on the active collection.
pub async fn run<S: SecretStore>(
    core: &Core<S>,
    request: EnvCompareRequest,
) -> Result<EnvComparison, AppError> {
    let _busy = Busy::start()?;
    let mut snapshots = Vec::new();
    let mut id = String::new();
    for environment in &request.environments {
        // Never trusted from the webview, as at send: the keychain is the only source.
        let input = SendInput {
            environment: Some(environment.clone()),
            vars: Vec::new(),
            secrets: Vec::new(),
        };
        let (snapshot, collection) = core.run_snapshot(input).await?;
        id = collection;
        snapshots.push(snapshot);
    }
    let parts = Arc::clone(&core.services);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        let runs: Vec<EnvRun> = snapshots
            .into_iter()
            .map(|snapshot| EnvRun {
                snapshot,
                services: services.services(),
            })
            .collect();
        runtime.block_on(compare_environments(
            &runs,
            &request,
            &transport,
            &options,
            &|| false,
        ))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the comparison failed: {e}")))?
}

#[tauri::command]
pub async fn envcompare_run(
    core: State<'_, AppCore>,
    request: EnvCompareRequest,
) -> Result<EnvComparison, CommandError> {
    Ok(run(&core, request).await?)
}
