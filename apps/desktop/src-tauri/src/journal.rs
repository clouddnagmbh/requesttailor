// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The test-data journal for the desktop app (`Backend.journal`, `rt_app::journal`), one journal
//! per collection in the data folder (`journal/<collection id>.jsonl`, shared with `rt
//! journal`). Plans and executions read the collection as it is now (unsaved edits, session
//! variables, secret values from the keychain and external sources, never the webview's) and
//! send through the app's services, so the send guard, cookies and tokens apply as for any
//! send. Audit entries go to `agents/audit.jsonl`.

use std::sync::Arc;

use rt_app::journal::{
    self, CompensationItem, EntryStates, FileStorage, Journal, JournalPage, JournalQuery,
    JournalSettings, ManualRevert, PluginCompensationRule, PreRead, QueueStorage, RevertEnv,
    RevertOptions, RevertOutcome, RevertPlan,
};
use rt_app::runner::RunSnapshot;
use rt_app::{AppError, SecretStore, SendInput};
use rt_core::model::RequestSpec;
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// The journal of the active collection (opened on first use) and the collection id.
fn journal_of<S: SecretStore>(core: &Core<S>) -> Result<(Arc<Journal>, String), AppError> {
    let data = core.data_dir.clone();
    core.with_workspace(|ws, id| {
        let name = ws.collection().name.clone();
        let id = id.to_owned();
        let key = id.clone();
        let journal = journal::journal_for(&name, move || match &data {
            Some(dir) => Arc::new(FileStorage::new(dir, &key)),
            None => Arc::new(QueueStorage::default()),
        });
        Ok((journal, id))
    })
}

/// The collection for `environment` with its secrets, as a run sees it, and the collection id.
async fn snapshot_of<S: SecretStore>(
    core: &Core<S>,
    environment: Option<String>,
) -> Result<(RunSnapshot, String), AppError> {
    core.with_send_input(
        SendInput {
            environment,
            ..SendInput::default()
        },
        |ws, input, id| Ok((ws.run_snapshot(input), id.to_owned())),
    )
    .await
}

pub fn settings<S: SecretStore>(core: &Core<S>) -> Result<JournalSettings, AppError> {
    Ok(journal_of(core)?.0.settings())
}

pub fn set_settings<S: SecretStore>(
    core: &Core<S>,
    settings: JournalSettings,
) -> Result<(), AppError> {
    journal_of(core)?.0.set_settings(settings)?;
    journal::refresh();
    Ok(())
}

pub fn entries<S: SecretStore>(
    core: &Core<S>,
    query: &JournalQuery,
) -> Result<JournalPage, AppError> {
    Ok(journal_of(core)?.0.entries(query))
}

pub fn clear<S: SecretStore>(core: &Core<S>) -> Result<(), AppError> {
    let journal = journal_of(core)?.0;
    journal.clear();
    journal.flush();
    Ok(())
}

pub async fn plan_manual<S: SecretStore>(
    core: &Core<S>,
    revert: ManualRevert,
) -> Result<RevertPlan, AppError> {
    let (journal, _) = journal_of(core)?;
    let entry = journal
        .entry(revert.entry_id)
        .ok_or_else(|| AppError::Shell(format!("journal entry #{} not found", revert.entry_id)))?;
    let (snapshot, _) = snapshot_of(core, entry.environment.clone()).await?;
    journal::plan_manual(&snapshot, &entry, &revert.request_path)
}

pub async fn plan_compensations<S: SecretStore>(
    core: &Core<S>,
    items: Vec<CompensationItem>,
) -> Result<RevertPlan, AppError> {
    let (journal, _) = journal_of(core)?;
    let environment = items
        .first()
        .and_then(|i| journal.entry(i.entry_id))
        .and_then(|e| e.environment);
    let (snapshot, _) = snapshot_of(core, environment).await?;
    journal::plan_compensations(&snapshot, &journal, &items)
}

pub async fn execute<S: SecretStore>(
    core: &Core<S>,
    plan: RevertPlan,
    options: RevertOptions,
) -> Result<RevertOutcome, AppError> {
    let (journal, _) = journal_of(core)?;
    let (snapshot, id) = snapshot_of(core, plan.environment.clone()).await?;
    let parts = Arc::clone(&core.services);
    let data = core.data_dir.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let services = services.services();
        let send_options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        let env = RevertEnv {
            snapshot: &snapshot,
            transport: &transport,
            options: &send_options,
            services: &services,
            caller: "app",
        };
        let audit = |entry: &rt_app::agents::AuditEntry| {
            if let Some(dir) = &data {
                let _ = rt_app::agents::append_audit(dir, entry);
            }
        };
        let outcome = runtime.block_on(journal::execute(
            &env,
            Some(&journal),
            &plan,
            &options,
            &audit,
            &|| false,
        ));
        journal.flush();
        outcome
    })
    .await
    .map_err(|e| AppError::Shell(format!("the revert failed: {e}")))?
}

pub async fn pre_read<S: SecretStore>(
    core: &Core<S>,
    request: RequestSpec,
    environment: Option<String>,
) -> Result<PreRead, AppError> {
    let (journal, _) = journal_of(core)?;
    let (snapshot, id) = snapshot_of(core, environment).await?;
    let parts = Arc::clone(&core.services);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let services = services.services();
        let send_options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        let env = RevertEnv {
            snapshot: &snapshot,
            transport: &transport,
            options: &send_options,
            services: &services,
            caller: "app",
        };
        runtime.block_on(journal::pre_read(&env, &journal, &request))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the read failed: {e}")))?
}

pub fn states<S: SecretStore>(
    core: &Core<S>,
    entry_ids: &[u64],
) -> Result<Vec<EntryStates>, AppError> {
    Ok(journal_of(core)?.0.states(entry_ids))
}

#[tauri::command]
pub async fn journal_settings(core: State<'_, AppCore>) -> Result<JournalSettings, CommandError> {
    Ok(settings(&core)?)
}

#[tauri::command]
pub async fn journal_set_settings(
    core: State<'_, AppCore>,
    settings: JournalSettings,
) -> Result<(), CommandError> {
    Ok(set_settings(&core, settings)?)
}

#[tauri::command]
pub async fn journal_entries(
    core: State<'_, AppCore>,
    query: JournalQuery,
) -> Result<JournalPage, CommandError> {
    Ok(entries(&core, &query)?)
}

#[tauri::command]
pub async fn journal_clear(core: State<'_, AppCore>) -> Result<(), CommandError> {
    Ok(clear(&core)?)
}

#[tauri::command]
pub async fn journal_plan_manual(
    core: State<'_, AppCore>,
    revert: ManualRevert,
) -> Result<RevertPlan, CommandError> {
    Ok(plan_manual(&core, revert).await?)
}

#[tauri::command]
pub async fn journal_plan_compensations(
    core: State<'_, AppCore>,
    items: Vec<CompensationItem>,
) -> Result<RevertPlan, CommandError> {
    Ok(plan_compensations(&core, items).await?)
}

#[tauri::command]
pub async fn journal_execute(
    core: State<'_, AppCore>,
    plan: RevertPlan,
    options: RevertOptions,
) -> Result<RevertOutcome, CommandError> {
    Ok(execute(&core, plan, options).await?)
}

#[tauri::command]
pub async fn journal_plugin_rules(
    core: State<'_, AppCore>,
) -> Result<Vec<PluginCompensationRule>, CommandError> {
    Ok(journal::plugin_rules(&core.services.plugins.list()))
}

#[tauri::command]
pub async fn journal_pre_read(
    core: State<'_, AppCore>,
    request: RequestSpec,
    environment: Option<String>,
) -> Result<PreRead, CommandError> {
    Ok(pre_read(&core, request, environment).await?)
}

#[tauri::command]
pub async fn journal_states(
    core: State<'_, AppCore>,
    entry_ids: Vec<u64>,
) -> Result<Vec<EntryStates>, CommandError> {
    Ok(states(&core, &entry_ids)?)
}
