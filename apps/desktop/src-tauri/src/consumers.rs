// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Consumer contracts for the desktop app (`Backend.consumers`, `rt_app::consumers`, wave 19
//! strand F). Commands: `consumers_list` (the consumers the active collection's
//! `opencollection.yml` lists, read from disk: the list is the team's decision, unsaved edits
//! do not count) and `consumers_verify` (find each consumer: a folder next to the provider or
//! a shallow clone into `consumers/` of the data folder; run it against `base_url` with the
//! app's services, so the send guard answers per request, and the secret values of its
//! environment from the keychain, never the webview's). One run at a time. Nothing is stored
//! except clones; nothing is sent without this command.

use std::sync::atomic::{AtomicBool, Ordering};

use rt_app::consumers::{
    Budget, ConsumerRef, ConsumerReport, ConsumerResult, ConsumerVerifyRequest, Loaded, fetch,
    run_consumer,
};
use rt_app::{AppError, SecretKey, SecretStore};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use rt_format::FileSet;
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
            return Err(AppError::Shell(
                "a consumer verification is already in progress".into(),
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

fn provider_root<S: SecretStore>(core: &Core<S>) -> Result<std::path::PathBuf, AppError> {
    core.root()
        .ok_or_else(|| AppError::Shell("open a collection first".into()))
}

/// The consumers the active collection lists.
pub async fn list<S: SecretStore>(core: &Core<S>) -> Result<Vec<ConsumerRef>, AppError> {
    let root = provider_root(core)?;
    tokio::task::spawn_blocking(move || {
        let files = FileSet::from_dir(&root).map_err(|e| AppError::Shell(e.to_string()))?;
        rt_app::consumers::list(&files)
    })
    .await
    .map_err(|e| AppError::Shell(format!("listing the consumers failed: {e}")))?
}

fn failed(consumer: &ConsumerRef, error: String) -> ConsumerResult {
    ConsumerResult {
        name: consumer.name.clone(),
        commit: None,
        requests: 0,
        skipped: 0,
        failures: Vec::new(),
        error: Some(error),
    }
}

/// Verifies the consumers of the active collection against `request.base_url`.
pub async fn verify<S: SecretStore + Clone + Send + 'static>(
    core: &Core<S>,
    request: ConsumerVerifyRequest,
) -> Result<ConsumerReport, AppError> {
    let _busy = Busy::start()?;
    let root = provider_root(core)?;
    let parts = std::sync::Arc::clone(&core.services);
    let data = core.data_dir.clone();
    let secrets = core.secrets().clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let files = FileSet::from_dir(&root).map_err(|e| AppError::Shell(e.to_string()))?;
        let listed = rt_app::consumers::list(&files)?;
        let chosen = rt_app::consumers::select(&listed, &request.only)?;
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        let token = std::env::var("RT_GIT_TOKEN").ok().filter(|t| !t.is_empty());
        let mut budget = Budget::new(request.max_minutes);
        let mut report = ConsumerReport::default();
        for consumer in chosen {
            let found = fetch::fetch(
                consumer,
                &root,
                data.as_deref(),
                request.fetch,
                token.as_deref(),
            );
            let result = match found.and_then(|found| {
                let files = FileSet::from_dir(&found.root)
                    .map_err(|e| format!("consumer {}: {e}", consumer.name))?;
                let doc = rt_format::read_collection(&files)
                    .map_err(|e| format!("consumer {}: {e}", consumer.name))?;
                let mut collection = doc.model().clone();
                let id = found
                    .root
                    .canonicalize()
                    .unwrap_or_else(|_| found.root.clone())
                    .to_string_lossy()
                    .into_owned();
                // The keychain, as for a send from that collection (external secret
                // managers are not read here).
                if let Some(env) = collection
                    .environments
                    .iter_mut()
                    .find(|e| e.name == consumer.environment)
                {
                    for var in env
                        .variables
                        .iter_mut()
                        .filter(|v| v.secret && v.value.is_none())
                    {
                        let key = SecretKey {
                            collection: id.clone(),
                            environment: consumer.environment.clone(),
                            variable: var.name.clone(),
                        };
                        if let Ok(Some(value)) = secrets.get(&key) {
                            var.value = Some(value);
                        }
                    }
                }
                Ok((
                    id,
                    Loaded {
                        collection,
                        files,
                        commit: found.commit,
                    },
                ))
            }) {
                Err(e) => failed(consumer, e),
                Ok((id, loaded)) => {
                    let services = parts.for_send(&id);
                    runtime.block_on(run_consumer(
                        consumer,
                        loaded,
                        &request,
                        &mut budget,
                        &transport,
                        &options,
                        &services.services(),
                        &|| false,
                    ))
                }
            };
            let broke = !result.failures.is_empty() || result.error.is_some();
            report.consumers.push(result);
            if broke && !request.keep_going {
                break;
            }
        }
        Ok(report)
    })
    .await
    .map_err(|e| AppError::Shell(format!("the verification failed: {e}")))?
}

#[tauri::command]
pub async fn consumers_list(core: State<'_, AppCore>) -> Result<Vec<ConsumerRef>, CommandError> {
    Ok(list(&core).await?)
}

#[tauri::command]
pub async fn consumers_verify(
    core: State<'_, AppCore>,
    request: ConsumerVerifyRequest,
) -> Result<ConsumerReport, CommandError> {
    Ok(verify(&core, request).await?)
}
