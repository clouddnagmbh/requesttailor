// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin catalogue in the web shell, over `rt_app::catalogue` (wave 18, strand I): the
//! same checks as the desktop app and `rt plugin`. Free functions of the module:
//!
//! ```text
//! catalogueLocate(source) -> { index_urls, commit, git }          // checks what the user typed
//! catalogueFetchIndex(source, companion | null) -> Promise<{ index: SourceIndex, index_url }>
//! cataloguePlan(source, id, installed | null, companion | null) -> Promise<InstallPlan>
//! catalogueModule(plan, installed | null, companion | null) -> Promise<Uint8Array>
//! catalogueUpdates(indexes: SourceIndex[], installed: InstalledSource[]) -> CatalogueUpdate[]
//! ```
//!
//! Nothing here is called except for a user action. A Git source is read through the host's
//! raw-file address only (no clone in the browser). `installed` is
//! `{ version, granted, sha256 | null }` of the plugin with the entry's id, if one is installed.
//! `catalogueModule` downloads the module again, repeats every check and refuses unless it is
//! the module the plan described; the shell then installs the bytes it returns.

use js_sys::{Promise, Uint8Array};
use rt_app::AppError;
use rt_app::catalogue::{
    CatalogueUpdate, InstallPlan, InstalledSource, InstalledState, SourceIndex, check_updates,
    fetch_index, fetch_wasm, locate, plan_install, resolve_wasm_url,
};
use rt_core::transport::Transport;
use rt_core::transport::companion::CompanionTransport;
use rt_core::transport::fetch::FetchTransport;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::Companion;
use crate::workspace_js::{app_error, from_js, to_js};

fn companion(value: &JsValue) -> Result<Option<Companion>, JsValue> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        Ok(Some(from_js(value, "companion")?))
    }
}

#[derive(Deserialize)]
struct Installed {
    version: String,
    granted: rt_app::plugins::Permissions,
    #[serde(default)]
    sha256: Option<String>,
}

fn installed_state(value: &JsValue) -> Result<Option<InstalledState>, JsValue> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let i: Installed = from_js(value, "installed")?;
    Ok(Some(InstalledState {
        version: i.version,
        granted: i.granted,
        sha256: i.sha256,
    }))
}

#[derive(Serialize)]
struct Located {
    index_urls: Vec<String>,
    commit: Option<String>,
    git: bool,
}

/// Checks a source the user typed; no network.
#[wasm_bindgen(js_name = catalogueLocate)]
pub fn catalogue_locate(source: String) -> Result<JsValue, JsValue> {
    let l = locate(&source).map_err(app_error)?;
    to_js(&Located {
        index_urls: l.index_urls,
        commit: l.commit,
        git: l.git.is_some(),
    })
}

async fn index_over<T: Transport>(t: &T, source: &str) -> Result<JsValue, JsValue> {
    let fetched = fetch_index(t, source, None).await.map_err(app_error)?;
    to_js(&fetched)
}

/// The index of `source`, read now (the user pressed Refresh).
#[wasm_bindgen(js_name = catalogueFetchIndex)]
pub fn catalogue_fetch_index(source: String, companion_js: JsValue) -> Result<Promise, JsValue> {
    let companion = companion(&companion_js)?;
    Ok(wasm_bindgen_futures::future_to_promise(async move {
        match companion {
            Some(c) => index_over(&CompanionTransport::new(c.url, c.token), &source).await,
            None => index_over(&FetchTransport::new(), &source).await,
        }
    }))
}

async fn plan_over<T: Transport>(
    t: &T,
    source: &str,
    id: &str,
    installed: Option<InstalledState>,
) -> Result<InstallPlan, AppError> {
    let fetched = fetch_index(t, source, None).await?;
    let entry = fetched
        .index
        .index
        .plugins
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Plugin(format!("{source} lists no plugin {id:?}")))?;
    let url = resolve_wasm_url(&fetched.index_url, entry)?;
    let bytes = fetch_wasm(t, &url).await?;
    plan_install(
        &fetched.index.source.url,
        &url,
        entry,
        &bytes,
        installed.as_ref(),
    )
}

/// What installing `id` from `source` would do (downloads the index and the module).
#[wasm_bindgen(js_name = cataloguePlan)]
pub fn catalogue_plan(
    source: String,
    id: String,
    installed: JsValue,
    companion_js: JsValue,
) -> Result<Promise, JsValue> {
    let companion = companion(&companion_js)?;
    let installed = installed_state(&installed)?;
    Ok(wasm_bindgen_futures::future_to_promise(async move {
        let plan = match companion {
            Some(c) => {
                plan_over(
                    &CompanionTransport::new(c.url, c.token),
                    &source,
                    &id,
                    installed,
                )
                .await
            }
            None => plan_over(&FetchTransport::new(), &source, &id, installed).await,
        };
        to_js(&plan.map_err(app_error)?)
    }))
}

async fn module_over<T: Transport>(
    t: &T,
    plan: &InstallPlan,
    installed: Option<InstalledState>,
) -> Result<Vec<u8>, AppError> {
    let bytes = fetch_wasm(t, &plan.wasm_url).await?;
    let again = plan_install(
        &plan.source,
        &plan.wasm_url,
        &plan.entry,
        &bytes,
        installed.as_ref(),
    )?;
    if again.sha256 != plan.sha256 {
        return Err(AppError::Plugin(format!(
            "{}: the module changed since you looked at it. Nothing was installed.",
            plan.entry.id
        )));
    }
    Ok(bytes)
}

/// The verified module of a confirmed plan (downloaded again, every check repeated).
#[wasm_bindgen(js_name = catalogueModule)]
pub fn catalogue_module(
    plan: JsValue,
    installed: JsValue,
    companion_js: JsValue,
) -> Result<Promise, JsValue> {
    let plan: InstallPlan = from_js(&plan, "plan")?;
    let companion = companion(&companion_js)?;
    let installed = installed_state(&installed)?;
    Ok(wasm_bindgen_futures::future_to_promise(async move {
        let bytes = match companion {
            Some(c) => {
                module_over(&CompanionTransport::new(c.url, c.token), &plan, installed).await
            }
            None => module_over(&FetchTransport::new(), &plan, installed).await,
        };
        Ok(Uint8Array::from(bytes.map_err(app_error)?.as_slice()).into())
    }))
}

/// Newer versions in `indexes` than what was installed from them (no network).
#[wasm_bindgen(js_name = catalogueUpdates)]
pub fn catalogue_updates(indexes: JsValue, installed: JsValue) -> Result<JsValue, JsValue> {
    let indexes: Vec<SourceIndex> = from_js(&indexes, "indexes")?;
    let installed: Vec<InstalledSource> = from_js(&installed, "installed")?;
    let updates: Vec<CatalogueUpdate> = check_updates(&indexes, &installed);
    to_js(&updates)
}
