// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Egress for extensions for the desktop app (`Backend.egress`, `rt_app::egress`, wave 19
//! strand B). One call is `rt_app::egress::request` with the app's native transport, the list
//! of hosts the user allowed on this device (`egress/hosts.json` in the data folder), the
//! send guard of the builder's `GuardHandle` (`Purpose::Egress`) and the secret values of the
//! active collection from the keychain; the webview never sees a value. Commands:
//! `egress_hosts`, `egress_allow`, `egress_remove`, `egress_request`.
//!
//! Nothing here touches the history, the response store or the exchange observers. Without a
//! data folder (tests) the list lives in memory for the process.

use std::sync::{Mutex, OnceLock};

use rt_app::egress::{
    self, Context, EgressError, EgressHost, EgressRequest, EgressResponse, HostError, HostList,
    file,
};
use rt_app::{AppError, SecretKey, SecretStore};
use rt_core::guard::Caller;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::Core;

/// The list when there is no data folder.
static MEMORY: Mutex<Option<HostList>> = Mutex::new(None);

fn transport() -> Result<&'static NativeTransport, EgressError> {
    static TRANSPORT: OnceLock<NativeTransport> = OnceLock::new();
    if let Some(t) = TRANSPORT.get() {
        return Ok(t);
    }
    let made = NativeTransport::new().map_err(|e| EgressError::Transport {
        message: e.to_string(),
    })?;
    Ok(TRANSPORT.get_or_init(|| made))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn shell(e: HostError) -> AppError {
    AppError::Shell(e.to_string())
}

/// What a command rejects with: the kind of the `EgressError` (`not-allowed`, `blocked`,
/// `limit`, `transport`) so the UI can tell them apart, and its readable message.
fn egress_error(e: &EgressError) -> CommandError {
    let kind = serde_json::to_value(e)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "transport".to_owned());
    CommandError {
        kind,
        message: e.to_string(),
    }
}

fn list_of<S: SecretStore>(core: &Core<S>) -> Result<HostList, AppError> {
    match &core.data_dir {
        Some(dir) => file::load(dir).map_err(shell),
        None => Ok(MEMORY
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .unwrap_or_default()),
    }
}

fn update<S: SecretStore, T>(
    core: &Core<S>,
    change: impl FnOnce(&mut HostList) -> Result<T, HostError>,
) -> Result<T, AppError> {
    match &core.data_dir {
        Some(dir) => file::update(dir, change).map_err(shell),
        None => {
            let mut guard = MEMORY.lock().unwrap_or_else(|p| p.into_inner());
            let list = guard.get_or_insert_with(HostList::new);
            change(list).map_err(shell)
        }
    }
}

/// The hosts allowed on this device.
pub fn hosts<S: SecretStore>(core: &Core<S>) -> Result<Vec<EgressHost>, AppError> {
    Ok(list_of(core)?.list().to_vec())
}

/// Allows `host` (and `port`) once for `extension` and `purpose`.
pub fn allow<S: SecretStore>(
    core: &Core<S>,
    host: &str,
    port: Option<u16>,
    extension: &str,
    purpose: &str,
) -> Result<EgressHost, AppError> {
    update(core, |list| {
        list.allow(host, port, extension, purpose, now_ms())
    })
}

/// Takes the permission for `host` and `port` back.
pub fn remove<S: SecretStore>(
    core: &Core<S>,
    host: &str,
    port: Option<u16>,
) -> Result<(), AppError> {
    update(core, |list| {
        list.remove(host, port);
        Ok(())
    })
}

/// The collection's name and the values of the secrets `request` names, from the keychain (and
/// the external secrets already read). Only the active collection has secrets here.
fn secrets_for<S: SecretStore>(
    core: &Core<S>,
    request: &EgressRequest,
) -> Result<(String, Vec<(String, String)>), EgressError> {
    let names = egress::secret_names(request);
    let wrong = |e: AppError| EgressError::Limit {
        message: e.to_string(),
    };
    let found = core.with_workspace(|workspace, id| {
        if let Some(asked) = &request.collection
            && asked != id
            && !names.is_empty()
        {
            return Err(AppError::Shell(
                "secrets of a collection that is not the active one are not available".into(),
            ));
        }
        let environments = workspace.environments();
        let values = egress::lookup_secrets(
            &names,
            &environments,
            request.environment.as_deref(),
            |environment, variable| {
                let key = SecretKey {
                    collection: id.to_owned(),
                    environment: environment.to_owned(),
                    variable: variable.to_owned(),
                };
                if let Some(value) = core.secrets().get(&key)? {
                    return Ok(Some(value));
                }
                Ok(environments
                    .iter()
                    .find(|e| e.name == environment)
                    .and_then(|e| e.external_secrets.as_ref())
                    .and_then(|external| {
                        core.external_secrets
                            .cached(external)
                            .into_iter()
                            .find(|(name, _)| name == variable)
                            .map(|(_, value)| value)
                    }))
            },
        )?;
        Ok((workspace.collection().name.clone(), values))
    });
    match found {
        Ok(found) => Ok(found),
        // No collection open: nothing to resolve; a request without secrets still works.
        Err(_) if names.is_empty() => Ok((String::new(), Vec::new())),
        Err(e) => Err(wrong(e)),
    }
}

/// One call. The list, the guard and the secrets are read first; the send runs on a blocking
/// thread (the guard is not `Send`).
pub async fn request<S: SecretStore + Send + Sync + 'static>(
    core: &Core<S>,
    request: EgressRequest,
) -> Result<EgressResponse, EgressError> {
    let hosts = list_of(core).map_err(|e| EgressError::Limit {
        message: e.to_string(),
    })?;
    let (collection, secrets) = secrets_for(core, &request)?;
    let parts = std::sync::Arc::clone(&core.services);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = transport()?;
        let ctx = Context {
            hosts: &hosts,
            guard: parts.guard.get(),
            caller: Caller::App,
            collection: &collection,
            environment: request.environment.as_deref(),
            secrets: &secrets,
        };
        runtime.block_on(egress::request(transport, &ctx, &request))
    })
    .await
    .map_err(|e| EgressError::Transport {
        message: format!("the call failed: {e}"),
    })?
}

#[tauri::command]
pub async fn egress_hosts(core: State<'_, AppCore>) -> Result<Vec<EgressHost>, CommandError> {
    Ok(hosts(&core)?)
}

#[tauri::command]
pub async fn egress_allow(
    core: State<'_, AppCore>,
    host: String,
    port: Option<u16>,
    extension: String,
    purpose: String,
) -> Result<EgressHost, CommandError> {
    Ok(allow(&core, &host, port, &extension, &purpose)?)
}

#[tauri::command]
pub async fn egress_remove(
    core: State<'_, AppCore>,
    host: String,
    port: Option<u16>,
) -> Result<(), CommandError> {
    Ok(remove(&core, &host, port)?)
}

#[tauri::command]
pub async fn egress_request(
    core: State<'_, AppCore>,
    request: EgressRequest,
) -> Result<EgressResponse, CommandError> {
    self::request(&core, request)
        .await
        .map_err(|e| egress_error(&e))
}
