// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The minimal failing request for the desktop app (`Backend.shrink`, `rt_app::shrink`, wave 19
//! strand E). The active collection's request is sent in variations (one send per probe, with
//! the app's services: send guard, cookies, tokens, plugins, each environment's secret values
//! from the keychain, never the webview's) until the smallest one that still fails is found.
//! Commands: `shrink_run`, `shrink_cancel`, `shrink_predicate`. One search at a time. Nothing
//! is stored; the result is a suggestion the UI opens as an unsaved draft.

use std::sync::atomic::{AtomicBool, Ordering};

use rt_app::shrink::{Predicate, ShrinkRequest, ShrinkResult, describe_failure, shrink};
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// The search in progress, if any.
static BUSY: AtomicBool = AtomicBool::new(false);
/// Set by `shrink_cancel`; polled before every probe.
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Marks the search as finished when dropped (also on errors and panics).
struct Busy;

impl Busy {
    fn start() -> Result<Self, AppError> {
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err(AppError::Shell(
                "a minimal-case search is already in progress".into(),
            ));
        }
        CANCELLED.store(false, Ordering::SeqCst);
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
    request: ShrinkRequest,
) -> Result<ShrinkResult, AppError> {
    let _busy = Busy::start()?;
    // Never trusted from the webview, as at send: the keychain is the only source.
    let input = SendInput {
        environment: request.environment.clone(),
        vars: Vec::new(),
        secrets: Vec::new(),
    };
    let (snapshot, id) = core.run_snapshot(input).await?;
    let parts = std::sync::Arc::clone(&core.services);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        runtime.block_on(shrink(
            &snapshot,
            &request,
            &transport,
            &options,
            &services.services(),
            &|| CANCELLED.load(Ordering::SeqCst),
        ))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the search failed: {e}")))?
}

/// What failed in the kept response `response_id` of `path`, as a predicate.
pub fn predicate<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    response_id: Option<u32>,
) -> Result<Predicate, AppError> {
    let id = response_id
        .ok_or_else(|| AppError::Shell(format!("{path} has no answer yet: send it first")))?;
    core.with_responses(|_, store| {
        let report = store.report(u64::from(id))?;
        describe_failure(report)
            .ok_or_else(|| AppError::Shell("The last answer did not fail.".into()))
    })
}

#[tauri::command]
pub async fn shrink_run(
    core: State<'_, AppCore>,
    request: ShrinkRequest,
) -> Result<ShrinkResult, CommandError> {
    Ok(run(&core, request).await?)
}

#[tauri::command]
pub async fn shrink_cancel() -> Result<(), CommandError> {
    CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn shrink_predicate(
    core: State<'_, AppCore>,
    path: String,
    response_id: Option<u32>,
) -> Result<Predicate, CommandError> {
    Ok(predicate(&core, &path, response_id)?)
}
