// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Paging for the desktop app (`Backend.paging`, `rt_app::paging`, wave 18 strand C). The rule
//! of a request is read from the active collection (companion, collection default) and from the
//! manifests of the enabled plugins; a run sends every page through the same services as a
//! normal send (send guard, cookies, tokens, plugins; secret values from the keychain, never the
//! webview's) and keeps the merged body in the response store. Commands: `paging_rule`,
//! `paging_set_rule`, `paging_run`.

use rt_app::paging::rules::{self, MatchContext};
use rt_app::paging::{
    EffectivePaging, PagedResult, PagingMode, PagingRuleScope, PagingRun, PagingSpec, PluginPaging,
    previous_page, run_on_snapshot,
};
use rt_app::{AppError, SecretStore};
use rt_core::send::SendOptions;
use rt_core::transport::native::NativeTransport;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::core::{BODY_LIMIT, Core};

/// The paging rules of the enabled plugins.
fn plugin_rules<S: SecretStore>(core: &Core<S>) -> Vec<PluginPaging> {
    core.services
        .plugins
        .list()
        .into_iter()
        .filter(|p| p.enabled)
        .flat_map(|p| {
            let plugin = p.manifest.id.clone();
            p.manifest
                .contributes
                .paging
                .into_iter()
                .map(move |contribution| PluginPaging {
                    plugin: plugin.clone(),
                    contribution,
                })
        })
        .collect()
}

/// What a plugin's rule is matched against: the kept response if there is one, else the
/// request as written.
fn context(
    ws: &rt_app::Workspace,
    store: &rt_app::ResponseStore,
    path: &str,
    response_id: Option<u64>,
) -> Option<MatchContext> {
    if let Some(report) = response_id.and_then(|id| store.report(id).ok()) {
        return Some(MatchContext {
            url: report
                .request
                .as_ref()
                .map(|r| r.url.clone())
                .unwrap_or_default(),
            content_type: report.response.as_ref().and_then(|r| {
                r.headers
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.clone())
            }),
        });
    }
    ws.request(path).ok().map(|spec| MatchContext {
        url: spec.url,
        content_type: None,
    })
}

pub fn rule<S: SecretStore>(
    core: &Core<S>,
    path: &str,
    response_id: Option<u64>,
) -> Result<Option<EffectivePaging>, AppError> {
    let plugins = plugin_rules(core);
    core.with_responses(|ws, store| {
        let ctx = context(ws, store, path, response_id);
        rules::effective(ws, path, &plugins, ctx.as_ref())
    })
}

pub async fn run<S: SecretStore>(core: &Core<S>, run: PagingRun) -> Result<PagedResult, AppError> {
    let plugins = plugin_rules(core);
    let (rule, previous, name, method) = core.with_responses(|ws, store| {
        let ctx = context(ws, store, &run.path, run.response_id);
        let rule = rules::effective(ws, &run.path, &plugins, ctx.as_ref())?
            .ok_or_else(|| AppError::Shell("This request has no paging rule.".into()))?;
        let previous = match run.mode {
            PagingMode::Next => {
                let id = run
                    .response_id
                    .ok_or_else(|| AppError::Shell("There is no page to continue from.".into()))?;
                Some(previous_page(store, id)?)
            }
            PagingMode::All => None,
        };
        let spec = ws.request(&run.path)?;
        Ok((rule, previous, spec.name, spec.method))
    })?;
    let (snapshot, id) = core.run_snapshot(run.input.clone()).await?;
    let parts = std::sync::Arc::clone(&core.services);
    let runtime = tokio::runtime::Handle::current();
    let request = run.clone();
    let paged = tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        let services = parts.for_send(&id);
        let options = SendOptions {
            body_limit: BODY_LIMIT,
            iteration: None,
        };
        runtime.block_on(run_on_snapshot(
            &snapshot,
            &request,
            &rule.spec,
            previous,
            &transport,
            &options,
            &services.services(),
            &|| false,
        ))
    })
    .await
    .map_err(|e| AppError::Shell(format!("paging failed: {e}")))??;
    let collection = core.with_workspace(|_, id| Ok(id.to_owned()))?;
    Ok(core.keep_paged(&collection, &name, &method, paged))
}

#[tauri::command]
pub async fn paging_rule(
    core: State<'_, AppCore>,
    path: String,
    response_id: Option<u64>,
) -> Result<Option<EffectivePaging>, CommandError> {
    Ok(rule(&core, &path, response_id)?)
}

#[tauri::command]
pub async fn paging_set_rule(
    core: State<'_, AppCore>,
    path: String,
    spec: Option<PagingSpec>,
    scope: PagingRuleScope,
) -> Result<(), CommandError> {
    Ok(core.with_workspace(|ws, _| rules::set(ws, &path, spec.as_ref(), scope))?)
}

#[tauri::command]
pub async fn paging_run(
    core: State<'_, AppCore>,
    run: PagingRun,
) -> Result<PagedResult, CommandError> {
    Ok(self::run(&core, run).await?)
}
