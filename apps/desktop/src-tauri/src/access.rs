// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The access check for the desktop app (`Backend.access`, `rt_app::access`, wave 19 strand D):
//! one request of the active collection is sent as user A (an environment) and, if the answer
//! holds an object, as user B, with the app's services (send guard, cookies, tokens, plugins)
//! and each environment's secret values from the keychain, never the webview's. Command:
//! `access_check`. One check at a time; every check leaves an entry in the audit log
//! (`agents/audit.jsonl`). Nothing else is stored.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rt_app::access::{AccessCheck, AccessEnv, AccessOptions, AccessProfile, check};
use rt_app::envcompare::EnvRun;
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// The check in progress, if any.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Marks the check as finished when dropped (also on errors and panics).
struct Busy;

impl Busy {
    fn start() -> Result<Self, AppError> {
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err(AppError::Shell(
                "an access check is already in progress".into(),
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

/// Runs the check of `path` on the active collection.
pub async fn run<S: SecretStore>(
    core: &Core<S>,
    path: String,
    profile: AccessProfile,
    options: AccessOptions,
) -> Result<AccessCheck, AppError> {
    let _busy = Busy::start()?;
    let mut snapshots = Vec::new();
    let mut id = String::new();
    for environment in [&profile.a, &profile.b] {
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
    let data = core.data_dir.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let send_options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        let mut snapshots = snapshots.into_iter();
        let (Some(a), Some(b)) = (snapshots.next(), snapshots.next()) else {
            return Err(AppError::Shell(
                "the access check needs two profiles".into(),
            ));
        };
        let a = EnvRun {
            snapshot: a,
            services: services.services(),
        };
        let b = EnvRun {
            snapshot: b,
            services: services.services(),
        };
        let audit = |entry: &rt_app::agents::AuditEntry| {
            if let Some(dir) = &data {
                let _ = rt_app::agents::append_audit(dir, entry);
            }
        };
        let env = AccessEnv {
            a: &a,
            b: &b,
            transport: &transport,
            options: &send_options,
            caller: "app",
            audit: &audit,
            cancelled: &|| false,
        };
        runtime.block_on(check(&env, &path, &profile, &options))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the access check failed: {e}")))?
}

#[tauri::command]
pub async fn access_check(
    core: State<'_, AppCore>,
    path: String,
    profile: AccessProfile,
    options: AccessOptions,
) -> Result<AccessCheck, CommandError> {
    Ok(run(&core, path, profile, options).await?)
}
