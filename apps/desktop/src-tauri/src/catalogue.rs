// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The plugin catalogue for the desktop app (`Backend.catalogue`, `rt_app::catalogue`). The
//! sources and where each installed plugin came from live in the plugin folder, shared with
//! `rt plugin`; nothing is requested except in answer to a command from the window (refresh,
//! plan, install, check for updates). A module is installed only after its SHA-256 matched
//! the index, and an install downloads it again and checks it against the plan the user
//! confirmed.

use rt_app::AppError;
use rt_app::catalogue::disk::DiskCatalogue;
use rt_app::catalogue::{
    CatalogueSource, CatalogueUpdate, Fetched, GitSource, InstallPlan, InstalledSource,
    SourceIndex, check_updates, fetch_index, fetch_wasm, plan_install, resolve_wasm_url,
};
use rt_core::transport::Transport;
use rt_core::transport::native::NativeTransport;
use rt_plugin::host::dir::Origin;
use tauri::State;

use crate::commands::{AppCore, CommandError};
use crate::plugins::PluginParts;

fn disk(parts: &PluginParts) -> Result<DiskCatalogue, AppError> {
    parts.dir().cloned().map(DiskCatalogue::new).ok_or_else(|| {
        AppError::Unsupported("The plugin catalogue needs the app's data folder.".into())
    })
}

/// Reads a Git source by cloning it: the bytes of `plugins.json` and the commit.
fn clone_index(git: &GitSource) -> Result<(Vec<u8>, String), String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let parent = std::env::temp_dir().join(format!(
        "rt-plugin-source-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let result = (|| {
        let folder = rt_git::clone(
            &git.url,
            &parent,
            &rt_git::NoCredentials,
            &rt_git::Options::default(),
        )
        .map_err(|e| e.to_string())?;
        let repo = rt_git::Repo::open(&folder, &rt_git::Options::default())
            .map_err(|e| e.to_string())?
            .ok_or("the clone is not a repository")?;
        let want = git.git_ref.as_deref().unwrap_or("HEAD");
        let commit = repo
            .rev_parse(want)
            .or_else(|_| repo.rev_parse(&format!("origin/{want}")))
            .map_err(|e| e.to_string())?;
        let text = repo
            .show(&commit, "/plugins.json")
            .map_err(|e| e.to_string())?
            .ok_or("the repository has no plugins.json in its root")?;
        Ok((text.into_bytes(), commit))
    })();
    let _ = std::fs::remove_dir_all(&parent);
    result
}

pub fn sources(parts: &PluginParts) -> Result<Vec<CatalogueSource>, AppError> {
    Ok(disk(parts)?.sources())
}

pub fn add_source(parts: &PluginParts, url: &str) -> Result<CatalogueSource, AppError> {
    disk(parts)?.add_source(url)
}

pub fn remove_source(parts: &PluginParts, url: &str) -> Result<(), AppError> {
    disk(parts)?.remove_source(url)
}

pub fn installed(parts: &PluginParts) -> Result<Vec<InstalledSource>, AppError> {
    Ok(disk(parts)?.installed_sources())
}

async fn read_source<T: Transport>(
    store: &DiskCatalogue,
    t: &T,
    url: &str,
) -> Result<Fetched, AppError> {
    let fetched = fetch_index(t, url, Some(&clone_index)).await?;
    store.remember(&fetched.index.source)?;
    Ok(fetched)
}

/// Reads the index of one source (`Some`) or of every source (`None`). Only on a user action.
pub async fn refresh<T: Transport>(
    parts: &PluginParts,
    t: &T,
    source: Option<&str>,
) -> Result<Vec<SourceIndex>, AppError> {
    let store = disk(parts)?;
    let urls: Vec<String> = match source {
        Some(url) => {
            if !store.sources().iter().any(|s| s.url == url) {
                return Err(AppError::Plugin(format!(
                    "{url} is not one of your sources"
                )));
            }
            vec![url.to_owned()]
        }
        None => store.sources().into_iter().map(|s| s.url).collect(),
    };
    let mut out = Vec::new();
    for url in urls {
        out.push(read_source(&store, t, &url).await?.index);
    }
    Ok(out)
}

pub async fn plan<T: Transport>(
    parts: &PluginParts,
    t: &T,
    source: &str,
    id: &str,
) -> Result<InstallPlan, AppError> {
    let store = disk(parts)?;
    let fetched = read_source(&store, t, source).await?;
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
        store.installed_state(id).as_ref(),
    )
}

/// Installs what `plan` describes: the module is downloaded again, checked against the plan
/// the user confirmed (same SHA-256) and against the index entry, then installed.
pub async fn install<T: Transport>(
    parts: &PluginParts,
    t: &T,
    plan: &InstallPlan,
) -> Result<InstalledSource, AppError> {
    let store = disk(parts)?;
    let bytes = fetch_wasm(t, &plan.wasm_url).await?;
    let again = plan_install(
        &plan.source,
        &plan.wasm_url,
        &plan.entry,
        &bytes,
        store.installed_state(&plan.entry.id).as_ref(),
    )?;
    if again.sha256 != plan.sha256 {
        return Err(AppError::Plugin(format!(
            "{}: the module changed since you looked at it. Nothing was installed.",
            plan.entry.id
        )));
    }
    let info = parts.install_from(
        &bytes,
        Some(Origin {
            source: plan.source.clone(),
            sha256: again.sha256.clone(),
        }),
    )?;
    Ok(InstalledSource {
        plugin: info.manifest.id,
        source: plan.source.clone(),
        version: info.manifest.version,
        sha256: again.sha256,
    })
}

/// Newer versions in the sources than what was installed from them; reads every source that
/// an installed plugin came from.
pub async fn updates<T: Transport>(
    parts: &PluginParts,
    t: &T,
) -> Result<Vec<CatalogueUpdate>, AppError> {
    let store = disk(parts)?;
    let installed = store.installed_sources();
    let mut seen: Vec<String> = Vec::new();
    let mut indexes = Vec::new();
    for i in &installed {
        if seen.contains(&i.source) {
            continue;
        }
        seen.push(i.source.clone());
        indexes.push(read_source(&store, t, &i.source).await?.index);
    }
    Ok(check_updates(&indexes, &installed))
}

/// Runs `f` with a native transport on a blocking thread (the clone of a Git source blocks).
async fn native<R, F, Fut>(f: F) -> Result<R, AppError>
where
    R: Send + 'static,
    F: FnOnce(NativeTransport) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<R, AppError>>,
{
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let transport = NativeTransport::new().map_err(|e| AppError::Shell(e.to_string()))?;
        runtime.block_on(f(transport))
    })
    .await
    .map_err(|e| AppError::Shell(format!("the catalogue call failed: {e}")))?
}

#[tauri::command]
pub async fn catalogue_sources(
    core: State<'_, AppCore>,
) -> Result<Vec<CatalogueSource>, CommandError> {
    Ok(sources(&core.services.plugins)?)
}

#[tauri::command]
pub async fn catalogue_add_source(
    core: State<'_, AppCore>,
    url: String,
) -> Result<CatalogueSource, CommandError> {
    Ok(add_source(&core.services.plugins, &url)?)
}

#[tauri::command]
pub async fn catalogue_remove_source(
    core: State<'_, AppCore>,
    url: String,
) -> Result<(), CommandError> {
    Ok(remove_source(&core.services.plugins, &url)?)
}

#[tauri::command]
pub async fn catalogue_refresh(
    core: State<'_, AppCore>,
    source: Option<String>,
) -> Result<Vec<SourceIndex>, CommandError> {
    let services = core.services.clone();
    Ok(
        native(move |t| async move { refresh(&services.plugins, &t, source.as_deref()).await })
            .await?,
    )
}

#[tauri::command]
pub async fn catalogue_plan(
    core: State<'_, AppCore>,
    source: String,
    id: String,
) -> Result<InstallPlan, CommandError> {
    let services = core.services.clone();
    Ok(native(move |t| async move { plan(&services.plugins, &t, &source, &id).await }).await?)
}

#[tauri::command]
pub async fn catalogue_install(
    core: State<'_, AppCore>,
    plan: InstallPlan,
) -> Result<InstalledSource, CommandError> {
    let services = core.services.clone();
    Ok(native(move |t| async move { install(&services.plugins, &t, &plan).await }).await?)
}

#[tauri::command]
pub async fn catalogue_installed(
    core: State<'_, AppCore>,
) -> Result<Vec<InstalledSource>, CommandError> {
    Ok(installed(&core.services.plugins)?)
}

#[tauri::command]
pub async fn catalogue_check_updates(
    core: State<'_, AppCore>,
) -> Result<Vec<CatalogueUpdate>, CommandError> {
    let services = core.services.clone();
    Ok(native(move |t| async move { updates(&services.plugins, &t).await }).await?)
}
